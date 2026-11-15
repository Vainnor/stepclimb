// SPDX-License-Identifier: AGPL-3.0-or-later
//! The OAuth 2.1 authorization endpoint over the router (docs/specs/m0/08-sessions-and-csrf.md
//! B30, B31, B41, Q10; T27 to T29).
//!
//! The app's side is played by the test: it opens `GET /oauth/authorize` with a PKCE S256
//! challenge, follows the browser through sign-in and the confirmation page, and reads the code
//! off the redirect to its loopback URI. The requests name their organization with the
//! `test-support` header of `request_org`, so a test can hold two. The rate limit walk over
//! every `/oauth/` route is `tests/enrollment.rs`'s. Needs `DATABASE_URL` (`just up`).

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
    password,
    token::{ApiKey, TokenHash},
};
use stepclimb_core::{
    OrgScope, Source,
    audit::Actor,
    clock::ManualClock,
    id::Uuid,
    local_logins, organizations,
    service_accounts::{self, NewServiceAccount},
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState, flow, request_org::TEST_ORG_HEADER, router, session, web_origin::WebOrigin,
};
use tower::ServiceExt;
use url::Url;

const AUTHORIZE: &str = "/oauth/authorize";
const PASSWORD: &str = "correct horse battery staple";
/// RFC 7636 appendix B's challenge.
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const REDIRECT: &str = "http://127.0.0.1:43210/oauth/callback";
const STATE: &str = "app state/1";
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

fn query(pairs: &[(&str, &str)]) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    for (name, value) in pairs {
        query.append_pair(name, value);
    }
    query.finish()
}

/// The app's authorization request, with `pairs` replacing or (with an empty value removing) its
/// parameters.
fn authorize_uri(changes: &[(&str, Option<&str>)]) -> String {
    let mut pairs = vec![
        ("response_type", "code"),
        ("client_id", "stepclimb-app"),
        ("redirect_uri", REDIRECT),
        ("code_challenge", CHALLENGE),
        ("code_challenge_method", "S256"),
        ("state", STATE),
    ];
    for (name, value) in changes {
        pairs.retain(|(n, _)| n != name);
        if let Some(value) = value {
            pairs.push((name, value));
        }
    }
    format!("{AUTHORIZE}?{}", query(&pairs))
}

#[derive(Debug)]
struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    text: String,
}

impl Answer {
    fn location(&self) -> Option<&str> {
        self.headers
            .get(header::LOCATION)
            .map(|v| v.to_str().expect("ASCII"))
    }

    /// The `name=value` pair of the `Set-Cookie` that sets (not clears) `name`.
    fn cookie(&self, name: &str) -> Option<String> {
        self.set_cookies()
            .into_iter()
            .filter(|v| v.starts_with(&format!("{name}=")) && !v.contains("Max-Age=0"))
            .map(|v| v.split(';').next().expect("a pair").to_owned())
            .next()
    }

    fn clears(&self, name: &str) -> bool {
        self.set_cookies()
            .iter()
            .any(|v| v.starts_with(&format!("{name}=; Max-Age=0")))
    }

    fn set_cookies(&self) -> Vec<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().expect("ASCII").to_owned())
            .collect()
    }

    /// The query of the redirect to the app's URI, by name.
    fn redirect_query(&self) -> Vec<(String, String)> {
        let location = self.location().expect("a redirect");
        assert!(location.starts_with(REDIRECT), "{location}");
        Url::parse(location)
            .expect("a URL")
            .query_pairs()
            .into_owned()
            .collect()
    }

    /// Whether this is the error page for `code`.
    fn is_page(&self, status: StatusCode, code: &str) -> bool {
        self.status == status
            && self.location().is_none()
            && self.headers[header::CONTENT_TYPE] == "text/html; charset=utf-8"
            && self.text.contains(&format!("<code>{code}</code>"))
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.text).expect("a JSON body")
    }
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
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
            passwords: Default::default(),
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

    /// A session cookie (`name=value`) for `user` of `org`.
    async fn session_in(&self, org: Uuid, user: Uuid) -> String {
        let started = session::start(
            &self.pool,
            scope(org),
            user,
            &HeaderMap::new(),
            self.clock.clock().now(),
        )
        .await
        .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        cookie.split(';').next().expect("a pair").to_owned()
    }

    async fn session(&self, user: Uuid) -> String {
        self.session_in(self.org, user).await
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
            .header(TEST_ORG_HEADER, org.to_string())
            .header("sec-fetch-site", "same-origin");
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

    /// `GET uri` in `org` with `cookies` (each `name=value`).
    async fn open_in(&self, org: Uuid, uri: &str, cookies: &[&str]) -> Answer {
        let cookie = cookies.join("; ");
        let headers: Vec<(&str, &str)> = if cookies.is_empty() {
            Vec::new()
        } else {
            vec![("cookie", cookie.as_str())]
        };
        self.send(Method::GET, uri, org, &headers, Body::empty())
            .await
    }

    async fn open(&self, uri: &str, cookies: &[&str]) -> Answer {
        self.open_in(self.org, uri, cookies).await
    }

    /// The confirmation form's `POST` in `org` with `cookies`.
    async fn answer_in(&self, org: Uuid, decision: &str, cookies: &[&str]) -> Answer {
        let cookie = cookies.join("; ");
        let headers = [
            ("cookie", cookie.as_str()),
            ("content-type", "application/x-www-form-urlencoded"),
            ("origin", "https://lms.example.org"),
        ];
        let body = Body::from(format!("decision={decision}"));
        self.send(Method::POST, AUTHORIZE, org, &headers, body)
            .await
    }

    async fn answer(&self, decision: &str, cookies: &[&str]) -> Answer {
        self.answer_in(self.org, decision, cookies).await
    }

    /// Open the app's request signed in as `session` and return the confirmation page's flow
    /// cookie.
    async fn confirmation(&self, session: &str) -> String {
        let page = self.open(&authorize_uri(&[]), &[session]).await;
        assert_eq!(page.status, StatusCode::OK, "{page:?}");
        page.cookie(flow::COOKIE_NAME).expect("a flow cookie")
    }

    async fn codes(&self) -> Vec<CodeRow> {
        sqlx::query_as(
            "SELECT org_id, user_id, client_id, code_hash, redirect_uri, code_challenge,
                    (extract(epoch FROM expires_at))::bigint
             FROM oauth_authorization_codes ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await
        .expect("read codes")
    }

    async fn flows(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM auth_flows")
            .fetch_one(&self.pool)
            .await
            .expect("count flows")
    }
}

type CodeRow = (Uuid, Uuid, String, Vec<u8>, String, String, i64);

/// T28 (B31): an unknown `client_id`, or a `redirect_uri` one character off, shows the error
/// page with its code and never redirects, signed in or not, and starts no flow.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t28_a_wrong_client_or_redirect_shows_the_page_and_never_redirects(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let session = f.session(sam).await;
    let cases = [
        (Some("other-app"), None, "oauth.client_unknown"),
        (None, None, "oauth.client_unknown"),
        (
            Some("stepclimb-app"),
            Some("http://127.0.0.1:43210/oauth/callbacks"),
            "oauth.redirect_uri_mismatch",
        ),
        (
            Some("stepclimb-app"),
            Some("http://127.0.0.2:43210/oauth/callback"),
            "oauth.redirect_uri_mismatch",
        ),
        (
            Some("stepclimb-app"),
            Some("https://evil.example/oauth/callback"),
            "oauth.redirect_uri_mismatch",
        ),
    ];
    for (client, redirect, code) in cases {
        let mut changes = vec![("client_id", client)];
        if redirect.is_some() {
            changes.push(("redirect_uri", redirect));
        }
        let uri = authorize_uri(&changes);
        for cookies in [vec![], vec![session.as_str()]] {
            let answer = f.open(&uri, &cookies).await;
            assert!(
                answer.is_page(StatusCode::BAD_REQUEST, code),
                "{uri}: {answer:?}"
            );
            assert!(answer.set_cookies().is_empty(), "{answer:?}");
        }
    }
    assert_eq!(f.flows().await, 0);
}

/// T27 (B30): without `code_challenge`, with `code_challenge_method=plain` or without one, or
/// with `response_type=token`, the browser goes back to the app with the RFC 6749 error and the
/// request's `state`, signed in or not; no flow and no code exist.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t27_no_pkce_s256_and_no_implicit_grant(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let session = f.session(sam).await;
    let cases = [
        (("code_challenge", None), "invalid_request"),
        (("code_challenge_method", Some("plain")), "invalid_request"),
        (("code_challenge_method", None), "invalid_request"),
        (("code_challenge", Some("abc")), "invalid_request"),
        (
            ("response_type", Some("token")),
            "unsupported_response_type",
        ),
    ];
    for (change, error) in cases {
        let uri = authorize_uri(&[change]);
        for cookies in [vec![], vec![session.as_str()]] {
            let answer = f.open(&uri, &cookies).await;
            assert_eq!(answer.status, StatusCode::SEE_OTHER, "{uri}: {answer:?}");
            let pairs = answer.redirect_query();
            assert_eq!(pairs[0], ("error".to_owned(), error.to_owned()), "{uri}");
            assert_eq!(pairs[1].0, "error_description");
            assert_eq!(pairs[2], ("state".to_owned(), STATE.to_owned()));
            assert_eq!(pairs.len(), 3);
            assert!(answer.cookie(flow::COOKIE_NAME).is_none());
        }
    }
    assert_eq!(f.flows().await, 0);
    assert!(f.codes().await.is_empty());
}

/// T29 (B31): without a session the browser goes to sign-in carrying the request; the password
/// sign-in returns it as its `return_to`; the request opened again shows the confirmation page,
/// and "Allow" sends the browser to the redirect URI with a code and the original `state`. The
/// code is stored only as its hash, bound to the organization, the user, the client, the
/// redirect URI and the challenge, for 60 seconds.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t29_sign_in_then_confirm_then_a_code_at_the_redirect_uri(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let hash = password::hash(PASSWORD).expect("a hash");
    local_logins::set_password(
        &f.pool,
        scope(f.org),
        sam,
        hash.as_str(),
        now(),
        Actor::Instance,
    )
    .await
    .expect("set the password");

    let request = authorize_uri(&[]);
    let first = f.open(&request, &[]).await;
    assert_eq!(first.status, StatusCode::SEE_OTHER, "{first:?}");
    let to_sign_in = Url::parse("https://lms.example.org")
        .unwrap()
        .join(first.location().expect("a redirect"))
        .unwrap();
    assert_eq!(to_sign_in.path(), "/sign-in");
    let (name, return_to) = to_sign_in.query_pairs().next().expect("return_to");
    assert_eq!(name, "return_to");
    assert_eq!(return_to, request, "the request, rebuilt in the same order");
    assert!(
        first.cookie(flow::COOKIE_NAME).is_none(),
        "no flow before sign-in"
    );

    let body = json!({ "email": "sam@example.test", "password": PASSWORD, "return_to": return_to });
    let signed_in = f
        .send(
            Method::POST,
            "/auth/local/password",
            f.org,
            &[("content-type", "application/json")],
            Body::from(body.to_string()),
        )
        .await;
    assert_eq!(signed_in.status, StatusCode::OK, "{signed_in:?}");
    assert_eq!(signed_in.json(), json!({ "return_to": request }));
    let session = signed_in.cookie(session::COOKIE_NAME).expect("a session");

    let page = f.open(&request, &[&session]).await;
    assert_eq!(page.status, StatusCode::OK, "{page:?}");
    assert!(
        page.text.contains("Sign in to the Stepclimb app?"),
        "{}",
        page.text
    );
    assert!(page.text.contains("<strong>Org A</strong>"));
    assert!(
        page.text
            .contains("<strong>Sam Example (sam@example.test)</strong>")
    );
    assert_eq!(page.headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(page.headers[header::X_FRAME_OPTIONS], "DENY");
    let flow_cookie = page.cookie(flow::COOKIE_NAME).expect("a flow cookie");
    assert!(f.codes().await.is_empty(), "no code before the answer");

    let allowed = f.answer("allow", &[&session, &flow_cookie]).await;
    assert_eq!(allowed.status, StatusCode::SEE_OTHER, "{allowed:?}");
    assert_eq!(allowed.headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(allowed.headers[header::REFERRER_POLICY], "no-referrer");
    assert!(allowed.clears(flow::COOKIE_NAME));
    let pairs = allowed.redirect_query();
    assert_eq!(pairs.len(), 2, "{pairs:?}");
    assert_eq!(pairs[0].0, "code");
    assert_eq!(pairs[1], ("state".to_owned(), STATE.to_owned()));
    let code = &pairs[0].1;
    assert_eq!(code.len(), 43);

    let rows = f.codes().await;
    assert_eq!(
        rows,
        [(
            f.org,
            sam,
            "stepclimb-app".to_owned(),
            TokenHash::of(code).as_bytes().to_vec(),
            REDIRECT.to_owned(),
            CHALLENGE.to_owned(),
            1_763_000_060,
        )]
    );
    let plaintext: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM oauth_authorization_codes
         WHERE position(convert_to($1, 'UTF8') in code_hash) > 0
            OR redirect_uri LIKE '%' || $1 || '%' OR code_challenge = $1",
    )
    .bind(code)
    .fetch_one(&f.pool)
    .await
    .expect("search for the code");
    assert_eq!(plaintext, 0);
    assert_eq!(f.flows().await, 0, "the answer used the flow");
}

/// Q10: "Cancel" sends the browser back with `access_denied` and the `state`, and issues no
/// code.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn declining_sends_access_denied_and_issues_no_code(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let session = f.session(sam).await;
    let flow_cookie = f.confirmation(&session).await;
    let declined = f.answer("deny", &[&session, &flow_cookie]).await;
    assert_eq!(declined.status, StatusCode::SEE_OTHER, "{declined:?}");
    let pairs = declined.redirect_query();
    assert_eq!(pairs[0], ("error".to_owned(), "access_denied".to_owned()));
    assert_eq!(pairs[2], ("state".to_owned(), STATE.to_owned()));
    assert!(f.codes().await.is_empty());
    assert_eq!(f.flows().await, 0);
}

/// B18: one confirmation answers once. A second answer with the same flow cookie, allow or
/// deny, shows `auth.flow_invalid` and issues nothing more.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_confirmation_is_answered_once(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let session = f.session(sam).await;
    let flow_cookie = f.confirmation(&session).await;
    let first = f.answer("allow", &[&session, &flow_cookie]).await;
    assert_eq!(first.status, StatusCode::SEE_OTHER, "{first:?}");
    for decision in ["allow", "deny"] {
        let again = f.answer(decision, &[&session, &flow_cookie]).await;
        assert!(
            again.is_page(StatusCode::BAD_REQUEST, "auth.flow_invalid"),
            "{again:?}"
        );
    }
    assert_eq!(f.codes().await.len(), 1);
}

/// B18, B21: a confirmation answered after the flow's 10 minutes shows `auth.flow_expired`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_late_answer_is_expired(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let session = f.session(sam).await;
    let flow_cookie = f.confirmation(&session).await;
    f.clock.advance(Duration::from_secs(10 * 60));
    let late = f.answer("allow", &[&session, &flow_cookie]).await;
    assert!(
        late.is_page(StatusCode::BAD_REQUEST, "auth.flow_expired"),
        "{late:?}"
    );
    assert!(f.codes().await.is_empty());
    assert_eq!(f.flows().await, 0, "the expired flow is deleted");
}

/// The code is bound to the user who saw the page: another user's session presenting the flow
/// cookie gets `auth.flow_invalid`, no code, and the flow is used up.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_users_session_cannot_answer_the_confirmation(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let kim = f.user("kim@example.test").await;
    let flow_cookie = f.confirmation(&f.session(sam).await).await;
    let kims = f.session(kim).await;
    let answer = f.answer("allow", &[&kims, &flow_cookie]).await;
    assert!(
        answer.is_page(StatusCode::BAD_REQUEST, "auth.flow_invalid"),
        "{answer:?}"
    );
    assert!(f.codes().await.is_empty());
    assert_eq!(f.flows().await, 0);
}

/// An answer without a session shows `auth.unauthenticated` and leaves the flow alone; with an
/// unknown session cookie it also clears the cookie.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_answer_needs_the_session(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let session = f.session(sam).await;
    let flow_cookie = f.confirmation(&session).await;
    let anonymous = f.answer("allow", &[&flow_cookie]).await;
    assert!(
        anonymous.is_page(StatusCode::UNAUTHORIZED, "auth.unauthenticated"),
        "{anonymous:?}"
    );
    let unknown = f
        .answer("allow", &["__Host-sc_session=unknown-token", &flow_cookie])
        .await;
    assert!(unknown.is_page(StatusCode::UNAUTHORIZED, "auth.unauthenticated"));
    assert!(unknown.clears(session::COOKIE_NAME));
    assert_eq!(f.flows().await, 1);
    assert!(f.codes().await.is_empty());
    let allowed = f.answer("allow", &[&session, &flow_cookie]).await;
    assert_eq!(allowed.status, StatusCode::SEE_OTHER, "{allowed:?}");
}

/// An answer that is not one `decision` of `allow` or `deny` shows `request.malformed` and
/// leaves the flow usable.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_malformed_answer_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let session = f.session(sam).await;
    let flow_cookie = f.confirmation(&session).await;
    for decision in ["yes", "allow&decision=allow", ""] {
        let answer = f.answer(decision, &[&session, &flow_cookie]).await;
        assert!(
            answer.is_page(StatusCode::BAD_REQUEST, "request.malformed"),
            "{decision}: {answer:?}"
        );
    }
    assert!(f.codes().await.is_empty());
    assert_eq!(f.flows().await, 1);
}

/// B14: the answer is a form post, so a cross-site one is refused by the CSRF check before it
/// runs; the flow stays usable.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_cross_site_answer_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let session = f.session(sam).await;
    let flow_cookie = f.confirmation(&session).await;
    let cookie = format!("{session}; {flow_cookie}");
    for (name, value) in [
        ("sec-fetch-site", "cross-site"),
        ("origin", "https://evil.example"),
    ] {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri(AUTHORIZE)
            .header(TEST_ORG_HEADER, f.org.to_string())
            .header("cookie", &cookie)
            .header("content-type", "application/x-www-form-urlencoded")
            .header(name, value)
            .body(Body::from("decision=allow"))
            .expect("a request");
        request.extensions_mut().insert(ConnectInfo(CLIENT));
        let response = f.app.clone().oneshot(request).await.expect("an answer");
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{name}");
    }
    assert!(f.codes().await.is_empty());
    assert_eq!(f.flows().await, 1);
}

/// B16, B44: a bearer request never authorizes an app, with a valid API key whose scopes cover
/// every permission and the owner's session cookie beside it: both methods answer 403
/// `auth.session_required` and no flow or code exists.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_bearer_request_never_authorizes_an_app(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let session = f.session(sam).await;
    let flow_cookie = f.confirmation(&session).await;
    let key = ApiKey::generate().expect("a key");
    let account = NewServiceAccount {
        name: "Every scope".into(),
        owner_user_id: sam,
        scopes: stepclimb_core::perm::ALL
            .iter()
            .map(|p| p.key().to_owned())
            .collect(),
    };
    service_accounts::create(
        &f.pool,
        scope(f.org),
        &account,
        key.hash().as_bytes(),
        now(),
        Actor::Instance,
    )
    .await
    .expect("create the key");
    let bearer = format!("Bearer {}", key.expose_secret());
    let cookie = format!("{session}; {flow_cookie}");
    let opened = f
        .send(
            Method::GET,
            &authorize_uri(&[]),
            f.org,
            &[("authorization", &bearer), ("cookie", &cookie)],
            Body::empty(),
        )
        .await;
    let answered = f
        .send(
            Method::POST,
            AUTHORIZE,
            f.org,
            &[
                ("authorization", &bearer),
                ("cookie", &cookie),
                ("content-type", "application/x-www-form-urlencoded"),
            ],
            Body::from("decision=allow"),
        )
        .await;
    for answer in [opened, answered] {
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{answer:?}");
        assert_eq!(answer.json()["code"], "auth.session_required");
        assert!(answer.location().is_none());
    }
    assert!(f.codes().await.is_empty());
    assert_eq!(f.flows().await, 1, "only the page's own flow");
}

/// B6: organization A's session presented in organization B is no session there: the request
/// goes to sign-in and starts no flow. Organization A's confirmation answered in organization B
/// with B's own user finds no flow, and A's session there is anonymous.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_session_and_flow_count_for_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org_b = f.other_org().await;
    let sam = f.user("sam@example.test").await;
    let kim = f.user_in(org_b, "kim@example.test").await;
    let sams = f.session(sam).await;
    let kims = f.session_in(org_b, kim).await;
    let flow_cookie = f.confirmation(&sams).await;

    let elsewhere = f.open_in(org_b, &authorize_uri(&[]), &[&sams]).await;
    assert_eq!(elsewhere.status, StatusCode::SEE_OTHER, "{elsewhere:?}");
    assert!(
        elsewhere
            .location()
            .unwrap()
            .starts_with("/sign-in?return_to=")
    );
    assert!(elsewhere.cookie(flow::COOKIE_NAME).is_none());

    let as_a = f.answer_in(org_b, "allow", &[&sams, &flow_cookie]).await;
    assert!(
        as_a.is_page(StatusCode::UNAUTHORIZED, "auth.unauthenticated"),
        "{as_a:?}"
    );
    let as_b = f.answer_in(org_b, "allow", &[&kims, &flow_cookie]).await;
    assert!(
        as_b.is_page(StatusCode::BAD_REQUEST, "auth.flow_invalid"),
        "{as_b:?}"
    );
    assert!(f.codes().await.is_empty());
    assert_eq!(f.flows().await, 1, "A's flow is still A's");
    let allowed = f.answer("allow", &[&sams, &flow_cookie]).await;
    assert_eq!(allowed.status, StatusCode::SEE_OTHER, "{allowed:?}");
    assert_eq!(f.codes().await[0].0, f.org);
}

/// B10: an expired session sends the browser to sign-in and clears the cookie.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_expired_session_goes_to_sign_in_and_is_cleared(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let session = f.session(sam).await;
    f.clock.advance(Duration::from_secs(12 * 60 * 60));
    let answer = f.open(&authorize_uri(&[]), &[&session]).await;
    assert_eq!(answer.status, StatusCode::SEE_OTHER, "{answer:?}");
    assert!(
        answer
            .location()
            .unwrap()
            .starts_with("/sign-in?return_to=")
    );
    assert!(answer.clears(session::COOKIE_NAME));
    assert_eq!(f.flows().await, 0);
}

/// Q10's 60 seconds: the periodic cleanup keeps a code until its expiry and deletes it then.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_cleanup_deletes_a_code_at_its_expiry(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let session = f.session(sam).await;
    let flow_cookie = f.confirmation(&session).await;
    f.answer("allow", &[&session, &flow_cookie]).await;
    flow::delete_expired(&f.pool, now() + Duration::from_secs(59))
        .await
        .expect("a pass");
    assert_eq!(f.codes().await.len(), 1);
    flow::delete_expired(&f.pool, now() + Duration::from_secs(60))
        .await
        .expect("a pass");
    assert!(f.codes().await.is_empty());
}

/// T3's authorization part (B4): the whole round trip logged at `trace` leaves neither the code
/// nor the flow or session token in the log.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_round_trip_logs_no_code_or_token(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sam = f.user("sam@example.test").await;
    let log = Captured::default();
    let writer = log.clone();
    let subscriber = stepclimb_server::logging::subscriber("trace", move || writer.clone());
    let guard = tracing::subscriber::set_default(subscriber);
    let session = f.session(sam).await;
    let flow_cookie = f.confirmation(&session).await;
    let allowed = f.answer("allow", &[&session, &flow_cookie]).await;
    drop(guard);
    let code = allowed.redirect_query()[0].1.clone();
    let text = log.text();
    assert!(text.contains(AUTHORIZE), "the capture missed the requests");
    for secret in [
        code.as_str(),
        session.split_once('=').unwrap().1,
        flow_cookie.split_once('=').unwrap().1,
        CHALLENGE,
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
