// SPDX-License-Identifier: AGPL-3.0-or-later
//! Browser sessions over HTTP (docs/specs/m0/08-sessions-and-csrf.md B7 to B11, B13, B16, Q4,
//! Q5).
//!
//! A sign-in calls [`start`], which ends the session the browser's old cookie names, generates a
//! new token, stores its hash and returns the `Set-Cookie` value for `__Host-sc_session`. A route
//! that needs a signed-in user takes the [`Authenticated`] extractor, which reads the cookie,
//! resolves the request's organization and asks `stepclimb_core::sessions::authenticate` for the
//! principal. A request with an `Authorization: Bearer` header is authenticated by that header
//! alone ([`crate::api_key`]), never by its cookie (B16). Sign-out calls [`end`]. The cookie's value is the token and never reaches a log line
//! or an error body.

use std::time::SystemTime;

use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use sqlx::{Acquire, PgConnection, Postgres};
use stepclimb_auth::token::{RandomError, Token, TokenHash};
use stepclimb_core::{
    OrgScope,
    id::Uuid,
    sessions::{self, Authentication, Session},
};

use crate::{
    AppState, api_key,
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
    start_on(&mut conn, scope, user_id, request_headers, now).await
}

/// [`start`] on a connection the caller holds, such as a sign-in's transaction (`&mut tx`). A
/// handler calls this one: its future is `Send` for any lifetime of the connection, which the
/// generic [`start`]'s is not.
pub async fn start_on(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
    request_headers: &HeaderMap,
    now: SystemTime,
) -> Result<Started, StartError> {
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
    let scope = request_organization(state, request_headers).await?;
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

/// The hash of the one session token the request presents, if it presents exactly one.
pub(crate) fn presented_hash(headers: &HeaderMap) -> Option<TokenHash> {
    match presented_cookie(headers) {
        Presented::One(token) => Some(TokenHash::of(token)),
        Presented::Absent | Presented::Ambiguous | Presented::NotAscii => None,
    }
}

/// A request a valid browser session (B8 to B11), API key (B26, B27) or app access token (B36)
/// authenticates.
///
/// `user_id` is the session's or app token's user, or the key's owner. This type carries no permissions: a key
/// acts with its owner's grants narrowed to its scopes, which only `require::Require` and
/// `require::AuthOnly` compute, so a handler that checks a permission takes one of those, never
/// this type alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Authenticated {
    pub user_id: Uuid,
    scope: OrgScope,
    via: Via,
}

/// The credential that authenticated a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// The `__Host-sc_session` cookie.
    Session,
    /// An `Authorization: Bearer sc_...` API key of this service account.
    ApiKey { service_account_id: Uuid },
    /// An `Authorization: Bearer` app access token of this refresh family (B36).
    AppToken { family_id: Uuid },
}

impl Authenticated {
    /// The user's organization, which every repository call of the request acts in.
    pub fn scope(&self) -> OrgScope {
        self.scope
    }

    /// The credential the request was authenticated by.
    pub fn via(&self) -> Via {
        self.via
    }

    pub(crate) fn by_api_key(user_id: Uuid, scope: OrgScope, service_account_id: Uuid) -> Self {
        Self {
            user_id,
            scope,
            via: Via::ApiKey { service_account_id },
        }
    }

    pub(crate) fn by_app_token(user_id: Uuid, scope: OrgScope, family_id: Uuid) -> Self {
        Self {
            user_id,
            scope,
            via: Via::AppToken { family_id },
        }
    }
}

/// An authenticated request and, for an API key, the key's scopes, which its owner's grants are
/// narrowed to (B27).
pub(crate) struct Credentialed {
    pub(crate) principal: Authenticated,
    /// `None` for a browser session or app token, which act with all of their user's grants.
    pub(crate) key_scopes: Option<Vec<String>>,
}

/// The 401 (or other failure) of [`Authenticated`], with the cookie cleared when the session
/// is gone or was never one (B10).
#[derive(Debug)]
pub struct Rejection {
    error: ApiError,
    clear_cookie: bool,
}

impl Rejection {
    /// The error the request is refused with.
    pub fn error(&self) -> &ApiError {
        &self.error
    }

    /// Whether the response clears the session cookie: the cookie named no live session.
    pub(crate) fn clears_cookie(&self) -> bool {
        self.clear_cookie
    }

    pub(crate) fn keep(error: ApiError) -> Self {
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

pub(crate) fn unauthorized(code: &'static str, detail: &'static str) -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, code, detail)
}

pub(crate) fn unauthenticated() -> ApiError {
    unauthorized(code::AUTH_UNAUTHENTICATED, "Sign in to use this operation.")
}

impl FromRequestParts<AppState> for Authenticated {
    type Rejection = Rejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(authenticate(parts, state).await?.principal)
    }
}

/// Authenticate the request by its bearer credential or its session cookie.
///
/// B16: a bearer request is authenticated by its bearer credential only, never by a cookie
/// beside it, and its cookie is left alone; `crate::api_key` answers it, handing an app access
/// token to `crate::app_token`. Any other request is
/// the session cookie's.
pub(crate) async fn authenticate(
    parts: &mut Parts,
    state: &AppState,
) -> Result<Credentialed, Rejection> {
    if csrf::carries_bearer(&parts.headers) {
        return api_key::authenticate(parts, state).await;
    }
    let principal = by_cookie(&parts.headers, state).await?;
    Ok(Credentialed {
        principal,
        key_scopes: None,
    })
}

/// The principal of the request's session cookie (B8 to B11), whatever else the request carries.
/// A route that only a browser session may reach and that answers a missing session itself (the
/// OAuth 2.1 authorization endpoint sends the browser to sign in) calls this after refusing a
/// bearer request; every other route takes [`Authenticated`] or `require::Require`.
pub(crate) async fn by_cookie(
    headers: &HeaderMap,
    state: &AppState,
) -> Result<Authenticated, Rejection> {
    let hash = match presented_cookie(headers) {
        Presented::Absent => return Err(Rejection::keep(unauthenticated())),
        // Neither can name a session: answered as an unknown token, without a lookup.
        Presented::Ambiguous | Presented::NotAscii => {
            return Err(Rejection::clear(unauthenticated()));
        }
        Presented::One(token) => TokenHash::of(token),
    };
    let request_org = request_organization(state, headers)
        .await
        .map_err(Rejection::keep)?;
    let now = state.clock.now();
    let authentication = sessions::authenticate(&state.db, request_org, hash.as_bytes(), now)
        .await
        .map_err(|error| Rejection::keep(ApiError::internal(&error)))?;
    match authentication {
        Authentication::Valid(principal) => Ok(Authenticated {
            user_id: principal.user_id,
            scope: principal.scope,
            via: Via::Session,
        }),
        Authentication::Expired => Err(Rejection::clear(unauthorized(
            code::AUTH_SESSION_EXPIRED,
            "The session has expired. Sign in again.",
        ))),
        Authentication::Suspended => Err(Rejection::keep(suspended())),
        Authentication::Unknown => Err(Rejection::clear(unauthenticated())),
    }
}

/// 401 `auth.suspended`: the credential's user, or the API key's owner, is suspended (B11, B27,
/// B36).
pub(crate) fn suspended() -> ApiError {
    unauthorized(code::AUTH_SUSPENDED, "The account is suspended.")
}

/// The organization the session, API key or app token must belong to (B6, B8, B26, B36): the
/// request's.
#[expect(
    clippy::disallowed_methods,
    reason = "spec 08 B8, B13, B26, B36: the credential extractor and sign-out look the session, key or app token up within the request's organization"
)]
pub(crate) async fn request_organization(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<OrgScope, ApiError> {
    request_org::resolve(&state.db, headers).await
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
