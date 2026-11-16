// SPDX-License-Identifier: AGPL-3.0-or-later
//! The OAuth 2.1 token and revocation endpoints and app access tokens over the router
//! (docs/specs/m0/08-sessions-and-csrf.md B6, B11, B16, B32 to B37, Q11 to Q14; T3, T9, T27,
//! T30 to T34; `03-users-and-identities.md` T17's app-token case).
//!
//! The app's side is played by the test. Most tests store a code the way the authorization
//! endpoint does (`oauth_codes::create`, bound to the RFC 7636 appendix B challenge) and exchange
//! it; one walks the whole round trip from `GET /oauth/authorize`. The requests name their
//! organization with the `test-support` header of `request_org`, so a test can hold two. The
//! rate limit walk over every `/oauth/` route is `tests/enrollment.rs`'s. Needs `DATABASE_URL`
//! (`just up`).

use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_auth::{
    encryption::KeyRing,
    oauth_server::access_token::{self, Subject},
    token::Token,
};
use stepclimb_core::{
    OrgScope, Source,
    audit::{self, Actor},
    clock::ManualClock,
    id::{Uuid, new_id},
    oauth_codes::{self, NewCode},
    organizations, perm,
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    users::{self, NewUser, Status},
};
use stepclimb_server::{
    AppState, flow, openapi, request_org::TEST_ORG_HEADER, router, session, web_origin::WebOrigin,
};
use tower::ServiceExt;
use url::Url;

mod common;

const TOKEN: &str = "/oauth/token";
const REVOKE: &str = "/oauth/revoke";
const ME: &str = "/api/v1/me";
/// RFC 7636 appendix B.
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const REDIRECT: &str = "http://127.0.0.1:43210/oauth/callback";
const ISSUER: &str = "https://lms.example.org";
const KEYS: &str = "test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

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

fn form(pairs: &[(&str, &str)]) -> String {
    let mut form = url::form_urlencoded::Serializer::new(String::new());
    for (name, value) in pairs {
        form.append_pair(name, value);
    }
    form.finish()
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

    /// The RFC 6749 `error`, or the problem's `code`.
    fn error(&self) -> String {
        let body = self.json();
        body["error"]
            .as_str()
            .or_else(|| body["code"].as_str())
            .unwrap_or_default()
            .to_owned()
    }

    fn header(&self, name: header::HeaderName) -> Option<&str> {
        self.headers.get(name).map(|v| v.to_str().expect("ASCII"))
    }

    /// An RFC 6749 refusal with `error`, as section 5.2 shapes it, never cached.
    fn refused(&self, error: &str) -> bool {
        let body = self.json();
        self.status == StatusCode::BAD_REQUEST
            && body["error"] == error
            && body["error_description"].is_string()
            && body.as_object().map(|o| o.len()) == Some(2)
            && self.header(header::CACHE_CONTROL) == Some("no-store")
            && self.header(header::CONTENT_TYPE) == Some("application/json")
    }
}

/// A granted token response.
#[derive(Debug, Clone)]
struct Tokens {
    access: String,
    refresh: String,
}

struct Fixture {
    pool: PgPool,
    clock: ManualClock,
    app: Router,
    org: Uuid,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        let org = organizations::create(&pool, "org-a", "Org A", now())
            .await
            .expect("create organization")
            .id;
        let clock = ManualClock::new(now());
        let state = AppState {
            db: pool.clone(),
            clock: clock.clock(),
            keys: KeyRing::parse(KEYS).expect("a valid test key"),
            public_url: WebOrigin::parse(ISSUER).expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
            passwords: Default::default(),
            storage: None,
            providers: stepclimb_server::providers::Providers::new(false)
                .expect("provider clients"),
        };
        Self {
            pool,
            clock,
            app: router(state),
            org,
        }
    }

    async fn other_org(&self) -> Uuid {
        organizations::create(&self.pool, "org-b", "Org B", now())
            .await
            .expect("create organization")
            .id
    }

    async fn user_in(&self, org: Uuid, email: &str) -> Uuid {
        let new_user = NewUser {
            display_name: "Sam Example".into(),
            given_name: String::new(),
            family_name: String::new(),
            email: Some(email.to_owned()),
            locale: None,
            tz: "UTC".into(),
            source: Source::Local,
            external_id: None,
        };
        users::create(&self.pool, scope(org), &new_user, now())
            .await
            .expect("create user")
            .id
    }

    async fn user(&self, email: &str) -> Uuid {
        self.user_in(self.org, email).await
    }

    /// Give `user` every catalog permission at the organization, through a custom role.
    async fn make_admin(&self, user: Uuid) {
        let new_role = NewRole {
            key: "everything".into(),
            name: "Everything".into(),
            permissions: perm::ALL.iter().map(|p| p.key().to_owned()).collect(),
        };
        let role = roles::create_custom(
            &self.pool,
            scope(self.org),
            &new_role,
            now(),
            Actor::Instance,
        )
        .await
        .expect("create the role")
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
        .expect("assign the role");
    }

    async fn set_status(&self, org: Uuid, user: Uuid, status: Status) {
        sqlx::query("UPDATE users SET status = $3 WHERE org_id = $1 AND id = $2")
            .bind(org)
            .bind(user)
            .bind(status.as_str())
            .execute(&self.pool)
            .await
            .expect("set the status");
    }

    /// A code for `user` of `org`, stored as the authorization endpoint stores one.
    async fn code_in(&self, org: Uuid, user: Uuid) -> String {
        let code = Token::generate().expect("a code");
        let hash = code.hash();
        let new = NewCode {
            id: new_id(),
            user_id: user,
            client_id: "stepclimb-app",
            code_hash: hash.as_bytes(),
            redirect_uri: REDIRECT,
            code_challenge: CHALLENGE,
        };
        oauth_codes::create(&self.pool, scope(org), &new, self.clock.clock().now())
            .await
            .expect("store a code");
        code.expose_secret().to_owned()
    }

    async fn code(&self, user: Uuid) -> String {
        self.code_in(self.org, user).await
    }

    async fn send(
        &self,
        method: Method,
        uri: &str,
        org: Uuid,
        headers: &[(&str, &str)],
        body: Body,
    ) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(TEST_ORG_HEADER, org.to_string());
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let mut request = request.body(body).expect("a request");
        request.extensions_mut().insert(ConnectInfo(CLIENT));
        let response = self
            .app
            .clone()
            .oneshot(request)
            .await
            .expect("the router never fails");
        let (status, headers) = (response.status(), response.headers().clone());
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("the body is readable")
            .to_bytes();
        Answer {
            status,
            headers,
            text: String::from_utf8(bytes.to_vec()).expect("UTF-8"),
        }
    }

    /// A form `POST` to `uri` in `org`, as the app's HTTP client sends it: no cookie, no
    /// `Origin`, no Fetch Metadata.
    async fn post_form_in(&self, org: Uuid, uri: &str, pairs: &[(&str, &str)]) -> Answer {
        let headers = [("content-type", "application/x-www-form-urlencoded")];
        self.send(Method::POST, uri, org, &headers, Body::from(form(pairs)))
            .await
    }

    async fn exchange_in(&self, org: Uuid, code: &str, changes: &[(&str, &str)]) -> Answer {
        let mut pairs = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT),
            ("client_id", "stepclimb-app"),
            ("code_verifier", VERIFIER),
        ];
        for (name, value) in changes {
            pairs.retain(|(n, _)| n != name);
            pairs.push((name, value));
        }
        self.post_form_in(org, TOKEN, &pairs).await
    }

    async fn exchange(&self, code: &str, changes: &[(&str, &str)]) -> Answer {
        self.exchange_in(self.org, code, changes).await
    }

    async fn refresh_in(&self, org: Uuid, refresh: &str) -> Answer {
        let pairs = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("client_id", "stepclimb-app"),
        ];
        self.post_form_in(org, TOKEN, &pairs).await
    }

    async fn refresh(&self, refresh: &str) -> Answer {
        self.refresh_in(self.org, refresh).await
    }

    async fn revoke_in(&self, org: Uuid, token: &str) -> Answer {
        let pairs = [("token", token), ("client_id", "stepclimb-app")];
        self.post_form_in(org, REVOKE, &pairs).await
    }

    async fn revoke(&self, token: &str) -> Answer {
        self.revoke_in(self.org, token).await
    }

    /// `GET /api/v1/me` in `org` with `Authorization: Bearer <access>`.
    async fn me_in(&self, org: Uuid, access: &str) -> Answer {
        let bearer = format!("Bearer {access}");
        self.send(
            Method::GET,
            ME,
            org,
            &[("authorization", bearer.as_str())],
            Body::empty(),
        )
        .await
    }

    async fn me(&self, access: &str) -> Answer {
        self.me_in(self.org, access).await
    }

    /// Exchange a new code for `user` and return the granted tokens.
    async fn tokens(&self, user: Uuid) -> Tokens {
        let code = self.code(user).await;
        granted(&self.exchange(&code, &[]).await)
    }

    async fn refresh_rows(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM oauth_refresh_tokens")
            .fetch_one(&self.pool)
            .await
            .expect("count refresh tokens")
    }

    async fn reuse_entries(&self) -> Vec<audit::Entry> {
        audit::list(&self.pool, scope(self.org), 1000)
            .await
            .expect("read audit entries")
            .into_iter()
            .filter(|entry| entry.action == "refresh_token.reuse")
            .collect()
    }

    fn advance(&self, by: Duration) {
        self.clock.advance(by);
    }
}

/// The tokens of a 200 token response, after checking its RFC 6749 section 5.1 shape.
fn granted(answer: &Answer) -> Tokens {
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.header(header::CACHE_CONTROL), Some("no-store"));
    assert_eq!(answer.header(header::PRAGMA), Some("no-cache"));
    assert_eq!(
        answer.header(header::CONTENT_TYPE),
        Some("application/json")
    );
    let body = answer.json();
    let mut keys: Vec<&String> = body.as_object().expect("an object").keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        ["access_token", "expires_in", "refresh_token", "token_type"]
    );
    assert_eq!(body["token_type"], "Bearer");
    assert_eq!(body["expires_in"], 900);
    let refresh = body["refresh_token"].as_str().expect("a refresh token");
    assert_eq!(refresh.len(), 43, "a Token");
    Tokens {
        access: body["access_token"]
            .as_str()
            .expect("an access token")
            .to_owned(),
        refresh: refresh.to_owned(),
    }
}

fn document() -> Value {
    serde_json::to_value(openapi::document()).expect("the document as JSON")
}

/// T30 (B32): a wrong verifier answers `invalid_grant` and leaves the code usable; the right one
/// returns tokens with `expires_in = 900`, and the access token answers `GET /api/v1/me` as its
/// user (spec 03 T17's app-token case); a second exchange of the code answers `invalid_grant`,
/// revokes the family the first started, so its refresh and access tokens stop, and is audited.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t30_a_code_is_exchanged_once_with_its_verifier(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let code = f.code(sam).await;
    let wrong = f
        .exchange(
            &code,
            &[(
                "code_verifier",
                "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXK",
            )],
        )
        .await;
    assert!(wrong.refused("invalid_grant"), "{wrong:?}");
    assert_eq!(f.refresh_rows().await, 0);

    let tokens = granted(&f.exchange(&code, &[]).await);
    let me = f.me(&tokens.access).await;
    assert_eq!(me.status, StatusCode::OK, "{me:?}");
    assert_eq!(me.json()["user"]["id"], sam.to_string());
    let errors = common::schema_errors(&document(), "Me", &me.json());
    assert!(errors.is_empty(), "{errors:?}");
    assert!(f.reuse_entries().await.is_empty());

    let again = f.exchange(&code, &[]).await;
    assert!(again.refused("invalid_grant"), "{again:?}");
    assert!(f.refresh(&tokens.refresh).await.refused("invalid_grant"));
    let me = f.me(&tokens.access).await;
    assert_eq!(
        (me.status, me.error().as_str()),
        (StatusCode::UNAUTHORIZED, "auth.access_token_invalid")
    );
    let entries = f.reuse_entries().await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].actor, Actor::Instance);
    assert_eq!(entries[0].data["presented"], "authorization_code");
    assert_eq!(entries[0].data["user_id"], sam.to_string());
    assert_eq!(entries[0].data["client_address"], "192.0.2.10");
}

/// B32: the code must be exchanged by its client, for its redirect URI exactly, before it
/// expires, in its organization, for an active user. None of these failures spends the code.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_code_is_bound_to_its_client_redirect_organization_and_lifetime(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let other = f.other_org().await;
    let sam = f.user("sam@example.test").await;
    let code = f.code(sam).await;
    for redirect in [
        "http://127.0.0.1:43211/oauth/callback",
        "http://127.0.0.1:43210/oauth/callback/",
        "http://[::1]:43210/oauth/callback",
        "dev.stepclimb.shell:/oauth/callback",
    ] {
        let answer = f.exchange(&code, &[("redirect_uri", redirect)]).await;
        assert!(answer.refused("invalid_grant"), "{redirect}: {answer:?}");
    }
    let unknown = f.exchange(&code, &[("client_id", "other-app")]).await;
    assert!(unknown.refused("invalid_client"), "{unknown:?}");
    let elsewhere = f.exchange_in(other, &code, &[]).await;
    assert!(elsewhere.refused("invalid_grant"), "{elsewhere:?}");
    f.set_status(f.org, sam, Status::Suspended).await;
    assert!(f.exchange(&code, &[]).await.refused("invalid_grant"));
    f.set_status(f.org, sam, Status::Active).await;
    assert_eq!(f.refresh_rows().await, 0);
    granted(&f.exchange(&code, &[]).await);

    let late = f.code(sam).await;
    f.advance(Duration::from_secs(59));
    let on_time = f.code(sam).await;
    f.advance(Duration::from_secs(1));
    assert!(f.exchange(&late, &[]).await.refused("invalid_grant"));
    granted(&f.exchange(&on_time, &[]).await);
}

/// T27 (B30): the password and client credentials grants, and any grant but the two, answer
/// `unsupported_grant_type`; a request that is not one form of the parameters answers
/// `invalid_request`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t27_only_the_code_and_refresh_grants_are_supported(pool: PgPool) {
    let f = Fixture::new(pool).await;
    for grant in ["password", "client_credentials", "implicit"] {
        let pairs = [
            ("grant_type", grant),
            ("client_id", "stepclimb-app"),
            ("username", "sam@example.test"),
            ("password", "x"),
        ];
        let answer = f.post_form_in(f.org, TOKEN, &pairs).await;
        assert!(
            answer.refused("unsupported_grant_type"),
            "{grant}: {answer:?}"
        );
    }
    let json = f
        .send(
            Method::POST,
            TOKEN,
            f.org,
            &[("content-type", "application/json")],
            Body::from(r#"{"grant_type":"refresh_token"}"#),
        )
        .await;
    assert!(json.refused("invalid_request"), "{json:?}");
    let big = format!("grant_type=refresh_token&pad={}", "x".repeat(5000));
    let too_big = f
        .send(
            Method::POST,
            TOKEN,
            f.org,
            &[("content-type", "application/x-www-form-urlencoded")],
            Body::from(big),
        )
        .await;
    assert!(too_big.refused("invalid_request"), "{too_big:?}");
    let charset = f
        .send(
            Method::POST,
            TOKEN,
            f.org,
            &[(
                "content-type",
                "application/x-www-form-urlencoded; charset=UTF-8",
            )],
            Body::from("grant_type=password"),
        )
        .await;
    assert!(charset.refused("unsupported_grant_type"), "{charset:?}");
    let repeated = f
        .post_form_in(
            f.org,
            TOKEN,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", "a"),
                ("refresh_token", "b"),
                ("client_id", "stepclimb-app"),
            ],
        )
        .await;
    assert!(repeated.refused("invalid_request"), "{repeated:?}");
}

/// T31 (B36): an access token works at 14m59s and answers 401 `auth.access_token_expired` at 15m.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t31_an_access_token_expires_after_fifteen_minutes(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let tokens = f.tokens(sam).await;
    f.advance(Duration::from_secs(14 * 60 + 59));
    assert_eq!(f.me(&tokens.access).await.status, StatusCode::OK);
    f.advance(Duration::from_secs(1));
    let expired = f.me(&tokens.access).await;
    assert_eq!(expired.status, StatusCode::UNAUTHORIZED, "{expired:?}");
    assert_eq!(expired.error(), "auth.access_token_expired");
    assert_eq!(
        expired.header(header::CACHE_CONTROL),
        Some("no-store"),
        "an /api/v1 answer"
    );
}

/// T32 (B33, B34): refreshing with R1 returns R2 in the same family, and R1 stops working;
/// refreshing with R1 again answers `invalid_grant`, and R2 and the family's access tokens stop
/// too. The reuse is audited.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t32_a_reused_refresh_token_revokes_its_family(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let first = f.tokens(sam).await;
    let unrelated = f.tokens(sam).await;
    f.advance(Duration::from_secs(600));
    let second = granted(&f.refresh(&first.refresh).await);
    assert_ne!(second.refresh, first.refresh);
    assert_ne!(second.access, first.access);
    assert_eq!(f.me(&second.access).await.status, StatusCode::OK);
    assert_eq!(f.me(&first.access).await.status, StatusCode::OK, "live");
    let rows: Vec<(Uuid, Option<Uuid>)> = sqlx::query_as(
        "SELECT family_id, rotated_from FROM oauth_refresh_tokens ORDER BY created_at, id",
    )
    .fetch_all(&f.pool)
    .await
    .expect("read tokens");
    assert_eq!(rows.len(), 3);

    let reused = f.refresh(&first.refresh).await;
    assert!(reused.refused("invalid_grant"), "{reused:?}");
    assert!(f.refresh(&second.refresh).await.refused("invalid_grant"));
    for access in [&first.access, &second.access] {
        let answer = f.me(access).await;
        assert_eq!(answer.error(), "auth.access_token_invalid", "{answer:?}");
    }
    assert_eq!(f.me(&unrelated.access).await.status, StatusCode::OK);
    granted(&f.refresh(&unrelated.refresh).await);
    let entries = f.reuse_entries().await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].data["presented"], "refresh_token");
    assert_eq!(entries[0].data["tokens_deleted"], 2);
}

/// T33 (B34, Q11 strict): two concurrent refreshes with R1 produce one success at most, and the
/// loser counts as reuse, so the family ends.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t33_concurrent_refreshes_succeed_once_and_end_the_family(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let first = f.tokens(sam).await;
    let (a, b) = tokio::join!(f.refresh(&first.refresh), f.refresh(&first.refresh));
    let wins = [&a, &b]
        .iter()
        .filter(|answer| answer.status == StatusCode::OK)
        .count();
    assert!(wins <= 1, "{a:?} {b:?}");
    for answer in [&a, &b] {
        assert!(
            answer.status == StatusCode::OK || answer.refused("invalid_grant"),
            "{answer:?}"
        );
    }
    assert_eq!(f.refresh_rows().await, 0, "the family is revoked");
    assert_eq!(f.reuse_entries().await.len(), 1);
}

/// T34 (B35): revoking R2 makes R2 and every other token of its family fail; revoking an
/// unknown token answers 200; revoking with an access token ends its family too.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t34_revoking_a_token_ends_its_family(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let first = f.tokens(sam).await;
    let second = granted(&f.refresh(&first.refresh).await);
    let kept = f.tokens(sam).await;
    let revoked = f.revoke(&second.refresh).await;
    assert_eq!(revoked.status, StatusCode::OK, "{revoked:?}");
    assert_eq!(revoked.text, "");
    assert_eq!(revoked.header(header::CACHE_CONTROL), Some("no-store"));
    assert!(f.refresh(&second.refresh).await.refused("invalid_grant"));
    for access in [&first.access, &second.access] {
        assert_eq!(f.me(access).await.error(), "auth.access_token_invalid");
    }
    assert!(
        f.reuse_entries().await.is_empty(),
        "a revocation is no reuse"
    );

    for unknown in ["unknown", second.refresh.as_str(), "eyJ.e30.x"] {
        assert_eq!(f.revoke(unknown).await.status, StatusCode::OK, "{unknown}");
    }
    assert_eq!(f.me(&kept.access).await.status, StatusCode::OK);
    assert_eq!(f.revoke(&kept.access).await.status, StatusCode::OK);
    assert_eq!(
        f.me(&kept.access).await.error(),
        "auth.access_token_invalid"
    );
    assert!(f.refresh(&kept.refresh).await.refused("invalid_grant"));
    assert_eq!(f.refresh_rows().await, 0);

    let missing = f
        .post_form_in(f.org, REVOKE, &[("client_id", "stepclimb-app")])
        .await;
    assert!(missing.refused("invalid_request"), "{missing:?}");
    let other = f
        .post_form_in(f.org, REVOKE, &[("token", "t"), ("client_id", "other")])
        .await;
    assert!(other.refused("invalid_client"), "{other:?}");
}

/// T9 (B11, B36): a suspended user's refresh token answers `invalid_grant` and their access token
/// 401 `auth.suspended`; after unsuspending, both work again.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_suspension_stops_app_tokens_and_unsuspending_restores_them(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let tokens = f.tokens(sam).await;
    f.set_status(f.org, sam, Status::Suspended).await;
    let me = f.me(&tokens.access).await;
    assert_eq!(
        (me.status, me.error().as_str()),
        (StatusCode::UNAUTHORIZED, "auth.suspended")
    );
    assert!(f.refresh(&tokens.refresh).await.refused("invalid_grant"));
    f.set_status(f.org, sam, Status::Active).await;
    assert_eq!(f.me(&tokens.access).await.status, StatusCode::OK);
    granted(&f.refresh(&tokens.refresh).await);
}

/// Q11 (b): a refresh token stops 30 days after it was issued, and a family 90 days after sign-in
/// however often it is refreshed.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn refresh_tokens_expire_and_a_family_ends_at_ninety_days(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let idle = f.tokens(sam).await;
    let mut current = f.tokens(sam).await;
    let day = Duration::from_secs(86_400);
    for _ in 0..3 {
        f.advance(29 * day);
        current = granted(&f.refresh(&current.refresh).await);
    }
    assert!(f.refresh(&idle.refresh).await.refused("invalid_grant"));
    // 87 days in: the last refresh's token expires with the family, at 90 days.
    f.advance(3 * day - Duration::from_secs(1));
    current = granted(&f.refresh(&current.refresh).await);
    f.advance(Duration::from_secs(1));
    assert!(f.refresh(&current.refresh).await.refused("invalid_grant"));
}

/// B6: a token is anonymous in another organization; a token of another organization's family
/// or user cannot be refreshed or revoked there.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organization_refuses_the_tokens(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let other = f.other_org().await;
    let sam = f.user("sam@example.test").await;
    f.user_in(other, "sam@example.test").await;
    let tokens = f.tokens(sam).await;
    let me = f.me_in(other, &tokens.access).await;
    assert_eq!(
        (me.status, me.error().as_str()),
        (StatusCode::UNAUTHORIZED, "auth.access_token_invalid")
    );
    assert!(
        f.refresh_in(other, &tokens.refresh)
            .await
            .refused("invalid_grant")
    );
    assert_eq!(
        f.revoke_in(other, &tokens.refresh).await.status,
        StatusCode::OK
    );
    assert_eq!(
        f.revoke_in(other, &tokens.access).await.status,
        StatusCode::OK
    );
    assert_eq!(f.me(&tokens.access).await.status, StatusCode::OK, "kept");
    granted(&f.refresh(&tokens.refresh).await);
}

/// B36: a bearer credential that is not a token this server signed answers 401
/// `auth.access_token_invalid`, as does one signed with another server's key or for another
/// issuer.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_forged_access_token_is_invalid(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let tokens = f.tokens(sam).await;
    let claims = access_token::verify(
        &KeyRing::parse(KEYS).expect("the key"),
        &tokens.access,
        ISSUER,
        now(),
    )
    .expect("the issued token verifies");
    let sign = |keys: &str, issuer: &str| {
        let subject = Subject {
            issuer,
            user_id: sam,
            org_id: f.org,
            client_id: "stepclimb-app",
            family_id: claims.fid,
        };
        access_token::issue(&KeyRing::parse(keys).expect("a key"), &subject, now())
            .expect("a token")
            .expose_secret()
            .to_owned()
    };
    let foreign = sign("test:ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8=", ISSUER);
    let other_issuer = sign(KEYS, "https://other.example");
    let same = sign(KEYS, ISSUER);
    assert_eq!(f.me(&same).await.status, StatusCode::OK, "the control");
    let mut altered = tokens.access.clone();
    altered.push('A');
    for bad in [foreign, other_issuer, altered, "x.y.z".to_owned()] {
        let answer = f.me(&bad).await;
        assert_eq!(
            (answer.status, answer.error().as_str()),
            (StatusCode::UNAUTHORIZED, "auth.access_token_invalid"),
            "{bad}"
        );
    }
}

/// Spec 08 B44 and #189: an app token is a bearer credential, so every operation the document
/// says answers `auth.session_required` refuses it with that 403, and the admin user routes
/// refuse it on its own user, whatever its user's permissions. A browser session of the same
/// admin is not refused (the control).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_app_token_is_refused_wherever_a_session_is_required(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    f.make_admin(sam).await;
    let tokens = f.tokens(sam).await;
    let bearer = format!("Bearer {}", tokens.access);
    let doc = document();
    let mut walked = Vec::new();
    for (path, item) in doc["paths"].as_object().expect("paths") {
        for (method, operation) in item.as_object().expect("a path item") {
            let Some(forbidden) = operation["responses"].get("403") else {
                continue;
            };
            let description = response_description(&doc, forbidden);
            if !description.contains("auth.session_required") {
                continue;
            }
            let uri = fill_params(path, sam);
            let method = Method::from_bytes(method.to_uppercase().as_bytes()).expect("a method");
            let body = request_body(operation);
            let headers = [
                ("authorization", bearer.as_str()),
                ("content-type", "application/json"),
            ];
            let answer = f
                .send(method.clone(), &uri, f.org, &headers, Body::from(body))
                .await;
            assert_eq!(
                (answer.status, answer.error().as_str()),
                (StatusCode::FORBIDDEN, "auth.session_required"),
                "{method} {uri}: {answer:?}"
            );
            walked.push(format!("{method} {path}"));
        }
    }
    for expected in [
        "PATCH /api/v1/users/{user_id}",
        "POST /api/v1/users/{user_id}/identities",
        "PUT /api/v1/me/password",
        "POST /api/v1/admin/service-accounts",
        "GET /oauth/authorize",
    ] {
        assert!(
            walked.iter().any(|w| w == expected),
            "{expected} in {walked:?}"
        );
    }
    let me = f.me(&tokens.access).await;
    assert_eq!(me.status, StatusCode::OK, "{me:?}");
}

/// The description of a response, following a `$ref` to the components.
fn response_description(doc: &Value, response: &Value) -> String {
    let resolved = match response["$ref"].as_str() {
        Some(reference) => {
            let name = reference.rsplit('/').next().expect("a name");
            &doc["components"]["responses"][name]
        }
        None => response,
    };
    resolved["description"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// `path` with `{user_id}` as `user` and every other parameter a new id.
fn fill_params(path: &str, user: Uuid) -> String {
    let mut out = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        let end = rest[start..].find('}').expect("a closing brace") + start;
        out.push_str(&rest[..start]);
        if &rest[start..=end] == "{user_id}" {
            out.push_str(&user.to_string());
        } else {
            out.push_str(&new_id().to_string());
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// A body that passes the operation's body parsing, where it has one: a user patch changing the
/// email, so the self-target rule is reached; `{}` for every other JSON body.
fn request_body(operation: &Value) -> String {
    match operation["operationId"].as_str() {
        Some("updateUser") => json!({ "email": "sam.new@example.test" }).to_string(),
        Some("createUserIdentity") => {
            json!({ "provider_key": "example", "subject": "s-1" }).to_string()
        }
        _ if operation.get("requestBody").is_some() => "{}".to_owned(),
        _ => String::new(),
    }
}

/// B16: the token and revocation endpoints take a cross-site form post (the app's HTTP client
/// sends whatever Fetch Metadata it likes) and answer it without CORS headers.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_token_endpoints_skip_the_csrf_check_and_send_no_cors_headers(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let code = f.code(sam).await;
    let body = form(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", REDIRECT),
        ("client_id", "stepclimb-app"),
        ("code_verifier", VERIFIER),
    ]);
    let cross_site = [
        ("content-type", "application/x-www-form-urlencoded"),
        ("sec-fetch-site", "cross-site"),
        ("origin", "https://evil.example"),
    ];
    let answer = f
        .send(Method::POST, TOKEN, f.org, &cross_site, Body::from(body))
        .await;
    let tokens = granted(&answer);
    assert!(
        !answer
            .headers
            .keys()
            .any(|name| name.as_str().starts_with("access-control-")),
        "{answer:?}"
    );
    let revoke = form(&[("token", &tokens.refresh), ("client_id", "stepclimb-app")]);
    let revoked = f
        .send(Method::POST, REVOKE, f.org, &cross_site, Body::from(revoke))
        .await;
    assert_eq!(revoked.status, StatusCode::OK, "{revoked:?}");
    let authorize = f
        .send(
            Method::POST,
            "/oauth/authorize",
            f.org,
            &cross_site,
            Body::from("decision=allow"),
        )
        .await;
    assert_eq!(
        (authorize.status, authorize.error().as_str()),
        (StatusCode::FORBIDDEN, "auth.csrf_refused"),
        "the authorization answer is still checked"
    );
}

/// B21: the periodic cleanup deletes a family once its newest token has expired.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_cleanup_deletes_expired_families(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    f.tokens(sam).await;
    flow::delete_expired(&f.pool, now() + Duration::from_secs(29 * 86_400))
        .await
        .expect("clean up");
    assert_eq!(f.refresh_rows().await, 1);
    flow::delete_expired(&f.pool, now() + Duration::from_secs(30 * 86_400))
        .await
        .expect("clean up");
    assert_eq!(f.refresh_rows().await, 0);
}

/// The whole round trip as the app sees it: the authorization request with a session, the
/// confirmation, the code at the redirect URI with `state` and `iss`, the exchange, an API call,
/// a refresh and the revocation. T3 (B4): the server's subscriber at `trace` logs none of the
/// code, the verifier, the session cookie or any token.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_the_round_trip_works_and_logs_no_secret(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let started = session::start(&f.pool, scope(f.org), sam, &HeaderMap::new(), now())
        .await
        .expect("a session");
    let session = started
        .set_cookie
        .to_str()
        .expect("ASCII")
        .split(';')
        .next()
        .expect("a pair")
        .to_owned();
    let log = Captured::default();
    let writer = log.clone();
    let subscriber = stepclimb_server::logging::subscriber("trace", move || writer.clone());
    let guard = tracing::subscriber::set_default(subscriber);

    let query = form(&[
        ("response_type", "code"),
        ("client_id", "stepclimb-app"),
        ("redirect_uri", REDIRECT),
        ("code_challenge", CHALLENGE),
        ("code_challenge_method", "S256"),
        ("state", "s-1"),
    ]);
    let page = f
        .send(
            Method::GET,
            &format!("/oauth/authorize?{query}"),
            f.org,
            &[("cookie", session.as_str())],
            Body::empty(),
        )
        .await;
    assert_eq!(page.status, StatusCode::OK, "{page:?}");
    let flow_cookie = page
        .headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().expect("ASCII"))
        .find(|v| v.starts_with("__Host-sc_flow="))
        .expect("the flow cookie")
        .split(';')
        .next()
        .expect("a pair")
        .to_owned();
    let marker = "name=\"request\" value=\"";
    let start = page.text.find(marker).expect("the hidden field") + marker.len();
    let request = &page.text[start..start + page.text[start..].find('"').expect("its end")];
    let cookies = format!("{session}; {flow_cookie}");
    let answered = f
        .send(
            Method::POST,
            "/oauth/authorize",
            f.org,
            &[
                ("cookie", cookies.as_str()),
                ("content-type", "application/x-www-form-urlencoded"),
                ("origin", ISSUER),
                ("sec-fetch-site", "same-origin"),
            ],
            Body::from(format!("decision=allow&request={request}")),
        )
        .await;
    assert_eq!(answered.status, StatusCode::SEE_OTHER, "{answered:?}");
    let location = Url::parse(answered.header(header::LOCATION).expect("a redirect")).expect("URL");
    let pairs: Vec<(String, String)> = location.query_pairs().into_owned().collect();
    assert_eq!(pairs[0].0, "code");
    assert_eq!(pairs[1], ("state".to_owned(), "s-1".to_owned()));
    assert_eq!(pairs[2], ("iss".to_owned(), ISSUER.to_owned()));
    let code = pairs[0].1.clone();

    let tokens = granted(&f.exchange(&code, &[]).await);
    assert_eq!(f.me(&tokens.access).await.status, StatusCode::OK);
    let next = granted(&f.refresh(&tokens.refresh).await);
    assert!(f.refresh(&tokens.refresh).await.refused("invalid_grant"));
    assert_eq!(f.revoke(&next.refresh).await.status, StatusCode::OK);
    drop(guard);

    let text = log.text();
    assert!(text.contains("\"level\":\"TRACE\"") || text.contains("\"level\":\"DEBUG\""));
    assert!(text.contains(TOKEN), "the capture missed the requests");
    let session_token = session.split_once('=').expect("a pair").1;
    for secret in [
        code.as_str(),
        VERIFIER,
        session_token,
        tokens.access.as_str(),
        tokens.refresh.as_str(),
        next.access.as_str(),
        next.refresh.as_str(),
    ] {
        assert!(!text.contains(secret), "the log holds a secret");
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

/// B32, B33: a code and a refresh token are the client's they were issued to. The registry holds
/// one client, so the test stores a family and a code for another client id directly; the app's
/// client cannot refresh, revoke or exchange them, and an access token signed for that client
/// authenticates nothing and revokes nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_token_of_another_client_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let refresh = Token::generate().expect("a token");
    let family = new_id();
    sqlx::query(
        "INSERT INTO oauth_refresh_tokens
             (id, org_id, client_id, user_id, token_hash, family_id, expires_at, created_at)
         VALUES ($1, $2, 'other-app', $3, $4, $1,
                 to_timestamp(1763086400), to_timestamp(1763000000))",
    )
    .bind(family)
    .bind(f.org)
    .bind(sam)
    .bind(refresh.hash().as_bytes().to_vec())
    .execute(&f.pool)
    .await
    .expect("store another client's token");
    assert!(
        f.refresh(refresh.expose_secret())
            .await
            .refused("invalid_grant")
    );
    assert_eq!(
        f.revoke(refresh.expose_secret()).await.status,
        StatusCode::OK
    );
    let subject = Subject {
        issuer: ISSUER,
        user_id: sam,
        org_id: f.org,
        client_id: "other-app",
        family_id: family,
    };
    let access = access_token::issue(&KeyRing::parse(KEYS).expect("the key"), &subject, now())
        .expect("a token");
    let me = f.me(access.expose_secret()).await;
    assert_eq!(me.error(), "auth.access_token_invalid", "{me:?}");
    assert_eq!(
        f.revoke(access.expose_secret()).await.status,
        StatusCode::OK
    );
    assert_eq!(f.refresh_rows().await, 1, "nothing was revoked");

    let code = Token::generate().expect("a code");
    let hash = code.hash();
    let other_client = NewCode {
        id: new_id(),
        user_id: sam,
        client_id: "other-app",
        code_hash: hash.as_bytes(),
        redirect_uri: REDIRECT,
        code_challenge: CHALLENGE,
    };
    oauth_codes::create(&f.pool, scope(f.org), &other_client, now())
        .await
        .expect("store another client's code");
    let exchanged = f.exchange(code.expose_secret(), &[]).await;
    assert!(exchanged.refused("invalid_grant"), "{exchanged:?}");
}

/// B32, B46: only a reuse that passes the client, redirect URI and
/// verifier checks revokes the family. A replay of a used code with a wrong verifier or
/// redirect URI, from someone who saw the code but not the verifier, is `invalid_grant` and
/// keeps the app signed in; the replay with the verifier revokes the family.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn only_a_reuse_with_the_verifier_revokes_the_family(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let code = f.code(sam).await;
    let tokens = granted(&f.exchange(&code, &[]).await);
    for change in [
        (
            "code_verifier",
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXK",
        ),
        ("redirect_uri", "http://127.0.0.1:43211/oauth/callback"),
    ] {
        let replay = f.exchange(&code, &[change]).await;
        assert!(replay.refused("invalid_grant"), "{change:?}: {replay:?}");
    }
    assert_eq!(f.me(&tokens.access).await.status, StatusCode::OK, "kept");
    assert!(f.reuse_entries().await.is_empty());
    assert_eq!(f.refresh_rows().await, 1);

    assert!(f.exchange(&code, &[]).await.refused("invalid_grant"));
    assert_eq!(f.refresh_rows().await, 0, "the verified replay revokes");
    assert_eq!(f.reuse_entries().await.len(), 1);
}
