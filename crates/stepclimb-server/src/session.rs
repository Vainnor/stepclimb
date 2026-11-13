// SPDX-License-Identifier: AGPL-3.0-or-later
//! Browser sessions over HTTP (docs/specs/m0/08-sessions-and-csrf.md B7 to B11, Q4, Q5).
//!
//! A sign-in calls [`start`], which generates a new token, stores its hash and returns the
//! `Set-Cookie` value for `__Host-sc_session`. A route that needs a signed-in user takes the
//! [`Authenticated`] extractor, which reads the cookie, resolves the request's organization and
//! asks `stepclimb_core::sessions::authenticate` for the principal. The cookie's value is the
//! token and never reaches a log line or an error body.

use std::time::SystemTime;

use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use sqlx::PgExecutor;
use stepclimb_auth::token::{RandomError, Token, TokenHash};
use stepclimb_core::{
    OrgScope,
    id::Uuid,
    sessions::{self, Authentication, Session},
};

use crate::{AppState, error::ApiError, request_org};

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
/// The token is always new: whatever session cookie the request carried is ignored, so a token
/// planted before sign-in is never promoted. `now` is the request's clock reading.
pub async fn start(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: Uuid,
    now: SystemTime,
) -> Result<Started, StartError> {
    let token = Token::generate()?;
    let session = sessions::create(db, scope, user_id, token.hash().as_bytes(), now).await?;
    let set_cookie = set_cookie(&token, session.expires_at);
    Ok(Started {
        session,
        set_cookie,
    })
}

/// `__Host-sc_session=<token>` with the B7 attributes and `Expires` at the session's absolute
/// expiry (Q5), so the cookie survives a browser restart and the server still enforces both
/// limits.
fn set_cookie(token: &Token, expires_at: SystemTime) -> HeaderValue {
    let cookie = format!(
        "{COOKIE_NAME}={}; Expires={}; {ATTRIBUTES}",
        token.expose_secret(),
        httpdate::fmt_http_date(expires_at),
    );
    // A token is base64url and an HTTP date is ASCII letters, digits, spaces, `,` and `:`, all
    // valid in a header value.
    HeaderValue::from_str(&cookie).expect("a session cookie is a valid header value")
}

/// The `Set-Cookie` value that removes the session cookie (B10). It repeats `Secure` and
/// `Path=/`, without which a browser refuses to touch a `__Host-` cookie.
pub fn clear_cookie() -> HeaderValue {
    HeaderValue::from_static(
        "__Host-sc_session=; Max-Age=0; Path=/; Secure; HttpOnly; SameSite=Lax",
    )
}

/// What the request's `Cookie` headers say about the session cookie.
#[derive(Debug, PartialEq, Eq)]
enum Presented<'a> {
    Absent,
    One(&'a str),
    /// More than one `__Host-sc_session`, which a browser never sends for one host; the request
    /// is not trusted to mean either.
    Ambiguous,
}

fn presented_cookie(headers: &HeaderMap) -> Presented<'_> {
    let mut values = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|line| line.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .filter(|(name, _)| *name == COOKIE_NAME)
        .map(|(_, value)| value);
    match (values.next(), values.next()) {
        (None, _) => Presented::Absent,
        (Some(value), None) => Presented::One(value),
        (Some(_), Some(_)) => Presented::Ambiguous,
    }
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
        let hash = match presented_cookie(&parts.headers) {
            Presented::Absent => return Err(Rejection::keep(unauthenticated())),
            Presented::Ambiguous => return Err(Rejection::clear(unauthenticated())),
            Presented::One(token) => TokenHash::of(token),
        };
        let request_org = request_org::resolve(&state.db)
            .await
            .map_err(Rejection::keep)?;
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
}
