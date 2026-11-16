// SPDX-License-Identifier: AGPL-3.0-or-later
//! An app signed in as a user, for the tests of the changes that must sign every app out (spec 08
//! B46: a new password, the use of an enrollment link). A test binary takes it with
//! `mod app_tokens;` and uses each function, so none is dead in any binary that takes it.
//!
//! [`sign_in`] stores an authorization code the way `/oauth/authorize` stores one and exchanges it
//! at `/oauth/token` with the RFC 7636 appendix B verifier; [`signed_in`] asks `GET /api/v1/me`
//! with the access token; [`signed_out`] checks the refresh token is refused and the access token
//! answers 401.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::SystemTime;

use axum::{
    Router,
    body::Body,
    extract::ConnectInfo,
    http::{Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::PgPool;
use stepclimb_auth::token::Token;
use stepclimb_core::{
    OrgScope,
    id::{Uuid, new_id},
    oauth_codes::{self, NewCode},
};
use stepclimb_server::request_org::TEST_ORG_HEADER;
use tower::ServiceExt;

const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const REDIRECT: &str = "http://127.0.0.1:43210/oauth/callback";
const APP_CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 99)), 50_000);

/// The tokens a signed-in app holds.
#[derive(Debug, Clone)]
pub struct AppTokens {
    pub access: String,
    pub refresh: String,
}

async fn send(
    app: &Router,
    org: Uuid,
    method: Method,
    uri: &str,
    bearer: Option<&str>,
    form: Option<String>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(TEST_ORG_HEADER, org.to_string());
    if let Some(token) = bearer {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let body = match form {
        Some(form) => {
            request = request.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
            Body::from(form)
        }
        None => Body::empty(),
    };
    let mut request = request.body(body).expect("a request");
    request.extensions_mut().insert(ConnectInfo(APP_CLIENT));
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router never fails");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("the body")
        .to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn form(pairs: &[(&str, &str)]) -> String {
    let mut form = url::form_urlencoded::Serializer::new(String::new());
    for (name, value) in pairs {
        form.append_pair(name, value);
    }
    form.finish()
}

/// Sign the app in as `user` of `org` (whose scope is `scope`) at `now`.
pub async fn sign_in(
    app: &Router,
    pool: &PgPool,
    scope: OrgScope,
    org: Uuid,
    user: Uuid,
    now: SystemTime,
) -> AppTokens {
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
    oauth_codes::create(pool, scope, &new, now)
        .await
        .expect("store a code");
    let body = form(&[
        ("grant_type", "authorization_code"),
        ("code", code.expose_secret()),
        ("redirect_uri", REDIRECT),
        ("client_id", "stepclimb-app"),
        ("code_verifier", VERIFIER),
    ]);
    let (status, answer) = send(app, org, Method::POST, "/oauth/token", None, Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    AppTokens {
        access: answer["access_token"]
            .as_str()
            .expect("an access token")
            .to_owned(),
        refresh: answer["refresh_token"]
            .as_str()
            .expect("a refresh token")
            .to_owned(),
    }
}

/// Whether the app's access token answers `GET /api/v1/me` as `user`.
pub async fn signed_in(app: &Router, org: Uuid, tokens: &AppTokens, user: Uuid) -> bool {
    let (status, answer) = send(
        app,
        org,
        Method::GET,
        "/api/v1/me",
        Some(&tokens.access),
        None,
    )
    .await;
    status == StatusCode::OK && answer["user"]["id"] == user.to_string()
}

/// Whether the app is signed out: its refresh token answers `invalid_grant` and its access token
/// 401 `auth.access_token_invalid`.
pub async fn signed_out(app: &Router, org: Uuid, tokens: &AppTokens) -> bool {
    let refresh = form(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &tokens.refresh),
        ("client_id", "stepclimb-app"),
    ]);
    let (refresh_status, refreshed) =
        send(app, org, Method::POST, "/oauth/token", None, Some(refresh)).await;
    let (me_status, me) = send(
        app,
        org,
        Method::GET,
        "/api/v1/me",
        Some(&tokens.access),
        None,
    )
    .await;
    refresh_status == StatusCode::BAD_REQUEST
        && refreshed["error"] == "invalid_grant"
        && me_status == StatusCode::UNAUTHORIZED
        && me["code"] == "auth.access_token_invalid"
}
