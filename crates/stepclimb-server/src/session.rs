// SPDX-License-Identifier: AGPL-3.0-or-later
//! Browser sessions over HTTP (docs/specs/m0/08-sessions-and-csrf.md B7 to B11, B13, B16, Q4,
//! Q5).
//!
//! A sign-in calls [`start`], which ends the session the browser's old cookie names, generates a
//! new token, stores its hash and returns the `Set-Cookie` value for `__Host-sc_session`. A route
//! that needs a signed-in user takes the [`Authenticated`] extractor, which reads the cookie,
//! resolves the request's organization and asks `stepclimb_core::sessions::authenticate` for the
//! principal. Sign-out calls [`end`]. The cookie's value is the token and never reaches a log line
//! or an error body.

use std::time::SystemTime;

use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use sqlx::{Acquire, Postgres};
use stepclimb_auth::token::{RandomError, Token, TokenHash};
use stepclimb_core::{
    OrgScope,
    id::Uuid,
    sessions::{self, Authentication, Session},
};

use crate::{
    AppState,
    cookie::{self, Presented},
    csrf,
    error::ApiError,
    request_org,
};

/// The session cookie's name. The `__Host-` prefix makes the browser refuse it unless it is
/// `Secure`, has `Path=/` and no `Domain`, so no other host can set or read it.
pub const COOKIE_NAME: &str = "__Host-sc_session";

/// The attributes every session cookie carries (B7).
const ATTRIBUTES: &str = "Path=/; Secure; HttpOnly; SameSite=Lax";

/// The codes this module produces (all 401).
pub mod code {
    /// No credential, or a session token that matches no session of the organization.
    pub const AUTH_UNAUTHENTICATED: &str = "auth.unauthenticated";
    /// The session passed its idle or absolute expiry.
    pub const AUTH_SESSION_EXPIRED: &str = "auth.session_expired";
    /// The credential's user is suspended.
    pub const AUTH_SUSPENDED: &str = "auth.suspended";
}

/// Why [`start`] could not create a session.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error(transparent)]
    Random(#[from] RandomError),
    #[error(transparent)]
    Session(#[from] sessions::Error),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// A session a sign-in has just created, and the cookie that hands its token to the browser.
#[derive(Debug)]
pub struct Started {
    pub session: Session,
    /// The `Set-Cookie` value. It holds the token: never log it.
    pub set_cookie: HeaderValue,
}

/// Create a browser session for the user `user_id` of the scope's organization (B7).
///
/// The token is always new, so a token planted before sign-in is never promoted. Every session
/// cookie `request_headers` carries is ended first: the session of this organization it names
/// is deleted, so signing in again leaves no second live session behind in the browser's old
/// cookie. Run it in the sign-in's transaction (B18) by passing `&mut tx`. `now` is the
/// request's clock reading.
pub async fn start<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    request_headers: &HeaderMap,
    now: SystemTime,
) -> Result<Started, StartError> {
    let mut conn = db.acquire().await?;
    for presented in session_cookies(request_headers).into_iter().flatten() {
        sessions::delete_by_token_hash(&mut *conn, scope, TokenHash::of(presented).as_bytes())
            .await?;
    }
    let token = Token::generate()?;
    let session =
        sessions::create(&mut *conn, scope, user_id, token.hash().as_bytes(), now).await?;
    let set_cookie = set_cookie(&token, session.expires_at);
    Ok(Started {
        session,
        set_cookie,
    })
}

/// End the session the request's cookie names, for sign-out (B13).
///
/// Every `__Host-sc_session` cookie the request carries has the session of the request's
/// organization it names deleted, live or expired, as [`start`] does. A request with no session
/// cookie, or with an `Authorization: Bearer` header (whose cookie names no session it may act
/// on, B16), deletes nothing and does not touch the database.
pub async fn end(state: &AppState, request_headers: &HeaderMap) -> Result<(), ApiError> {
    if csrf::carries_bearer(request_headers) {
        return Ok(());
    }
    let presented: Vec<&str> = session_cookies(request_headers)
        .into_iter()
        .flatten()
        .collect();
    if presented.is_empty() {
        return Ok(());
    }
    let scope = request_organization(state).await?;
    for token in presented {
        sessions::delete_by_token_hash(&state.db, scope, TokenHash::of(token).as_bytes())
            .await
            .map_err(|error| ApiError::internal(&error))?;
    }
    Ok(())
}

/// `__Host-sc_session=<token>` with the B7 attributes and `Expires` at the session's absolute
/// expiry (Q5), so the cookie survives a browser restart and the server still enforces both
/// limits. The value is marked sensitive so its `Debug` output hides the token.
fn set_cookie(token: &Token, expires_at: SystemTime) -> HeaderValue {
    let cookie = format!(
        "{COOKIE_NAME}={}; Expires={}; {ATTRIBUTES}",
        token.expose_secret(),
        httpdate::fmt_http_date(expires_at),
    );
    // A token is base64url and an HTTP date is ASCII letters, digits, spaces, `,` and `:`, all
    // valid in a header value.
    let mut value =
        HeaderValue::from_str(&cookie).expect("a session cookie is a valid header value");
    value.set_sensitive(true);
    value
}

/// The `Set-Cookie` value that removes the session cookie (B10). It repeats `Secure` and
/// `Path=/`, without which a browser refuses to touch a `__Host-` cookie.
pub fn clear_cookie() -> HeaderValue {
    HeaderValue::from_static(
        "__Host-sc_session=; Max-Age=0; Path=/; Secure; HttpOnly; SameSite=Lax",
    )
}

/// The value of every `__Host-sc_session` in the request's `Cookie` headers, `None` for one that
/// is not ASCII.
fn session_cookies(headers: &HeaderMap) -> Vec<Option<&str>> {
    cookie::values(headers, COOKIE_NAME)
}

fn presented_cookie(headers: &HeaderMap) -> Presented<'_> {
    cookie::presented(headers, COOKIE_NAME)
}

/// A request a valid browser session authenticates (B8 to B11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Authenticated {
    pub user_id: Uuid,
    scope: OrgScope,
}

impl Authenticated {
    /// The user's organization, which every repository call of the request acts in.
    pub fn scope(&self) -> OrgScope {
        self.scope
    }
}

/// The 401 (or other failure) of [`Authenticated`], with the cookie cleared when the session
/// is gone or was never one (B10).
#[derive(Debug)]
pub struct Rejection {
    error: ApiError,
    clear_cookie: bool,
}

impl Rejection {
    fn keep(error: ApiError) -> Self {
        Self {
            error,
            clear_cookie: false,
        }
    }

    fn clear(error: ApiError) -> Self {
        Self {
            error,
            clear_cookie: true,
        }
    }
}

impl IntoResponse for Rejection {
    fn into_response(self) -> Response {
        let mut response = self.error.into_response();
        if self.clear_cookie {
            response
                .headers_mut()
                .append(header::SET_COOKIE, clear_cookie());
        }
        response
    }
}

fn unauthorized(code: &'static str, detail: &'static str) -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, code, detail)
}

fn unauthenticated() -> ApiError {
    unauthorized(code::AUTH_UNAUTHENTICATED, "Sign in to use this operation.")
}

impl FromRequestParts<AppState> for Authenticated {
    type Rejection = Rejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // B16: a bearer request is authenticated by its bearer credential only, never by a
        // cookie beside it. Bearer credentials (API keys, app access tokens) are not accepted
        // yet, so such a request is anonymous; its cookie is left alone.
        if csrf::carries_bearer(&parts.headers) {
            return Err(Rejection::keep(unauthenticated()));
        }
        let hash = match presented_cookie(&parts.headers) {
            Presented::Absent => return Err(Rejection::keep(unauthenticated())),
            // Neither can name a session: answered as an unknown token, without a lookup.
            Presented::Ambiguous | Presented::NotAscii => {
                return Err(Rejection::clear(unauthenticated()));
            }
            Presented::One(token) => TokenHash::of(token),
        };
        let request_org = request_organization(state).await.map_err(Rejection::keep)?;
        let now = state.clock.now();
        let authentication = sessions::authenticate(&state.db, request_org, hash.as_bytes(), now)
            .await
            .map_err(|error| Rejection::keep(ApiError::internal(&error)))?;
        match authentication {
            Authentication::Valid(principal) => Ok(Self {
                user_id: principal.user_id,
                scope: principal.scope,
            }),
            Authentication::Expired => Err(Rejection::clear(unauthorized(
                code::AUTH_SESSION_EXPIRED,
                "The session has expired. Sign in again.",
            ))),
            Authentication::Suspended => Err(Rejection::keep(unauthorized(
                code::AUTH_SUSPENDED,
                "The account is suspended.",
            ))),
            Authentication::Unknown => Err(Rejection::clear(unauthenticated())),
        }
    }
}

/// The organization the session must belong to (B6, B8): the request's.
#[expect(
    clippy::disallowed_methods,
    reason = "spec 08 B8, B13: the session extractor and sign-out look the session up within the request's organization"
)]
async fn request_organization(state: &AppState) -> Result<OrgScope, ApiError> {
    request_org::resolve(&state.db).await
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    fn headers(cookies: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for cookie in cookies {
            headers.append(header::COOKIE, HeaderValue::from_str(cookie).unwrap());
        }
        headers
    }

    #[test]
    fn the_session_cookie_is_found_among_others() {
        let headers = headers(&["theme=dark; __Host-sc_session=abc-_1; lang=en"]);
        assert_eq!(presented_cookie(&headers), Presented::One("abc-_1"));
    }

    #[test]
    fn a_cookie_whose_name_only_contains_the_session_name_is_not_it() {
        let headers = headers(&["x__Host-sc_session=a; __Host-sc_session_old=b; sc_session=c"]);
        assert_eq!(presented_cookie(&headers), Presented::Absent);
        assert_eq!(presented_cookie(&HeaderMap::new()), Presented::Absent);
    }

    #[test]
    fn two_session_cookies_are_ambiguous_in_one_header_or_across_two() {
        let one = headers(&["__Host-sc_session=a; __Host-sc_session=b"]);
        let two = headers(&["__Host-sc_session=a", "__Host-sc_session=b"]);
        assert_eq!(presented_cookie(&one), Presented::Ambiguous);
        assert_eq!(presented_cookie(&two), Presented::Ambiguous);
    }

    #[test]
    fn a_non_ascii_neighbor_cookie_does_not_hide_the_session_cookie() {
        let mut headers = HeaderMap::new();
        headers.append(
            header::COOKIE,
            HeaderValue::from_bytes("theme=café; __Host-sc_session=abc".as_bytes()).unwrap(),
        );
        assert_eq!(presented_cookie(&headers), Presented::One("abc"));
    }

    #[test]
    fn a_non_ascii_session_value_names_no_session() {
        let mut headers = HeaderMap::new();
        headers.append(
            header::COOKIE,
            HeaderValue::from_bytes("__Host-sc_session=café".as_bytes()).unwrap(),
        );
        assert_eq!(presented_cookie(&headers), Presented::NotAscii);
        let mut latin1 = HeaderMap::new();
        latin1.append(
            header::COOKIE,
            HeaderValue::from_bytes(b"__Host-sc_session=caf\xe9").unwrap(),
        );
        assert_eq!(presented_cookie(&latin1), Presented::NotAscii);
    }

    #[test]
    fn the_set_cookie_carries_every_b7_attribute_and_the_expiry() {
        let token = Token::generate().unwrap();
        // 2025-12-13T02:13:20Z.
        let expires = UNIX_EPOCH + Duration::from_secs(1_765_592_000);
        let value = set_cookie(&token, expires);
        assert_eq!(
            value.to_str().unwrap(),
            format!(
                "__Host-sc_session={}; Expires=Sat, 13 Dec 2025 02:13:20 GMT; Path=/; Secure; \
                 HttpOnly; SameSite=Lax",
                token.expose_secret()
            )
        );
    }

    #[test]
    fn debug_output_of_a_started_session_never_contains_the_token() {
        let token = Token::generate().unwrap();
        let at = UNIX_EPOCH + Duration::from_secs(1_765_592_000);
        let started = Started {
            session: Session {
                id: Uuid::nil(),
                user_id: Uuid::nil(),
                last_seen_at: at,
                expires_at: at,
                created_at: at,
            },
            set_cookie: set_cookie(&token, at),
        };
        let printed = format!("{started:?}");
        assert!(!printed.contains(token.expose_secret()), "{printed}");
        assert!(started.set_cookie.is_sensitive());
    }
}
