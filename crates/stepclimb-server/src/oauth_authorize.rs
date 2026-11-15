// SPDX-License-Identifier: AGPL-3.0-or-later
//! The OAuth 2.1 authorization endpoint for Stepclimb's own apps: `GET` and `POST
//! /oauth/authorize` (docs/specs/m0/08-sessions-and-csrf.md B30, B31, B41, Q10).
//!
//! The app opens `GET /oauth/authorize` in the system browser. In order, the route:
//!
//! 1. counts the request against the client address's sign-in budget (B41); over it, the
//!    browser goes to the sign-in page with `error=auth.rate_limited`;
//! 2. refuses any request with a bearer header (403 `auth.session_required`): only a browser
//!    session authorizes an app, never an API key or another app's token;
//! 3. checks the request (`stepclimb_auth::oauth_server::authorize::parse`): a wrong client or
//!    redirect URI shows the error page and never redirects; any other problem goes back to the
//!    redirect URI with an RFC 6749 error and the request's `state`;
//! 4. reads the browser session. Without a valid one, the browser goes to the web app's
//!    sign-in page with the request as its `return_to` (a path back to this endpoint), which
//!    the sign-in completes to; no flow is started here, so sign-in flows never nest;
//! 5. signed in, it stores the request in an `oauth_authorize` sign-in flow for the user and
//!    shows the confirmation page naming the app, the user and the organization.
//!
//! The page's form posts the answer to `POST /oauth/authorize`, which passes the CSRF check
//! first (B14) like every unsafe request, then the same budget and bearer refusal, reads the
//! session again and, in one transaction, takes the flow (once: B18) and checks it is this
//! user's and its client and redirect URI still pass. "Cancel" sends the browser to the
//! redirect URI with `error=access_denied`; "Allow" stores a new authorization code, bound to
//! the organization, user, client, redirect URI and challenge and valid 60 seconds, and sends
//! the browser to the redirect URI with `code` and `state`. The code exists only in that
//! `Location` and as its hash in the table; no log line carries it (the request span leaves the
//! query out, spec 08 B4).

use axum::{
    body::{Body, to_bytes},
    extract::{RawQuery, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use stepclimb_auth::{
    oauth_server::authorize::{
        self, ErrorCode, Pending, RedirectError, Refusal, Request, with_query,
    },
    token::Token,
};
use stepclimb_core::{
    OrgScope,
    auth_flows::Kind as FlowKind,
    id::{Uuid, new_id},
    oauth_codes::{self, NewCode},
    organizations, users,
};
use url::form_urlencoded;

use crate::{
    AppState,
    error::{ApiError, code::REQUEST_MALFORMED},
    flow::{self, FlowStart, code::AUTH_FLOW_INVALID},
    oauth_page,
    provider_sign_in::{NavigationLimited, SIGN_IN_PAGE},
    require::NoBearer,
    session::{self, Authenticated},
};

/// The endpoint's path.
pub const AUTHORIZE_PATH: &str = "/oauth/authorize";

/// The longest answer body read; the form sends `decision=allow` or `decision=deny`.
const MAX_ANSWER_LEN: usize = 1024;

/// The `error_description` of a declined request.
const DECLINED: &str = "The user declined to sign in to the app.";

crate::openapi::problem_responses! {
    AuthorizeForbidden => "`auth.session_required`: the request carries a bearer credential; only a signed-in browser session authorizes an app.",
    AnswerForbidden => "`auth.session_required`: the request carries a bearer credential; only a signed-in browser session authorizes an app. `auth.csrf_refused`: the request did not come from this site.",
}

/// Start an app's sign-in: the authorization request of the OAuth 2.1 code flow with PKCE.
#[utoipa::path(
    get,
    path = "/oauth/authorize",
    operation_id = "authorizeApp",
    tag = "auth",
    description = "A browser navigation the app opens in the system browser. Takes \
`response_type=code`, `client_id`, `redirect_uri`, `code_challenge`, `code_challenge_method=S256` \
and `state`; other parameters are ignored and none may be given twice. An unknown `client_id` \
(`oauth.client_unknown`) or a `redirect_uri` the client has not registered \
(`oauth.redirect_uri_mismatch`) answers an HTML error page and never redirects. Any other problem \
is a 303 to the redirect URI with `error` (`invalid_request`, `unsupported_response_type`), \
`error_description` and `state`. Without a browser session, a 303 to \
`/sign-in?return_to=<this request>`. Signed in, the confirmation page, whose form posts the \
answer to `POST /oauth/authorize`, with the `__Host-sc_flow` cookie holding the request. Over the \
sign-in budget, a 303 to `/sign-in?error=auth.rate_limited` with `Retry-After`.",
    responses(
        (
            status = 200,
            description = "The confirmation page naming the app, the user and the organization",
            content_type = "text/html",
            body = String,
            headers(
                ("Set-Cookie" = String, description = "The `__Host-sc_flow` cookie of the request"),
                ("Cache-Control" = String, description = "`no-store`")
            )
        ),
        (
            status = 303,
            description = "To the redirect URI with an RFC 6749 error, to the sign-in page with the request as `return_to`, or to `/sign-in?error=auth.rate_limited`",
            headers(
                ("Location" = String, description = "Where the browser goes"),
                ("Retry-After" = i64, description = "With `auth.rate_limited`: seconds to wait, at least 1")
            )
        ),
        (
            status = 400,
            description = "An HTML error page: `oauth.client_unknown` or `oauth.redirect_uri_mismatch`",
            content_type = "text/html",
            body = String
        ),
        (status = 403, response = AuthorizeForbidden),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn authorize_app(
    _limited: NavigationLimited,
    _: NoBearer,
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let request = match authorize::parse(query.as_deref().unwrap_or("")) {
        Ok(request) => request,
        Err(Refusal::Page(code)) => return Ok(oauth_page::refused(code)),
        Err(Refusal::Redirect(error)) => return Ok(to_client(&error.location())),
    };
    let principal = match signed_in(&state, &headers).await {
        SignedIn::Yes(principal) => principal,
        SignedIn::No { clear } => return Ok(to_sign_in(&request, clear)),
        SignedIn::Fault(response) => return Ok(response),
    };
    confirm(&state, principal, &request).await
}

/// What the request's session cookie says.
enum SignedIn {
    /// A valid browser session.
    Yes(Authenticated),
    /// No valid session; `clear`: the cookie named no live session and is removed.
    No { clear: bool },
    /// A server fault or an organization that cannot be resolved, answered as it is.
    Fault(Response),
}

/// The request's browser session (B8 to B11). A suspended user's session counts as none.
async fn signed_in(state: &AppState, headers: &HeaderMap) -> SignedIn {
    match session::by_cookie(headers, state).await {
        Ok(principal) => SignedIn::Yes(principal),
        Err(rejection) if rejection.error().status() == StatusCode::UNAUTHORIZED => SignedIn::No {
            clear: rejection.clears_cookie(),
        },
        Err(rejection) => SignedIn::Fault(rejection.into_response()),
    }
}

/// The 303 to the sign-in page carrying the request back to this endpoint as `return_to`.
fn to_sign_in(request: &Request, clear_session: bool) -> Response {
    let query = form_urlencoded::Serializer::new(String::new())
        .append_pair("return_to", &return_to(request))
        .finish();
    let mut response = to_client(&format!("{SIGN_IN_PAGE}?{query}"));
    if clear_session {
        response
            .headers_mut()
            .append(header::SET_COOKIE, session::clear_cookie());
    }
    response
}

/// The path back to this endpoint with the request's parameters (spec 08 B31).
fn return_to(request: &Request) -> String {
    format!("{AUTHORIZE_PATH}?{}", request.to_query())
}

/// A 303 to `location`, with the pages' protective headers.
fn to_client(location: &str) -> Response {
    let mut response = crate::provider_sign_in::redirect(location);
    oauth_page::protect(&mut response);
    response
}

/// Store the request for the user's answer and show the confirmation page.
async fn confirm(
    state: &AppState,
    principal: Authenticated,
    request: &Request,
) -> Result<Response, ApiError> {
    let scope = principal.scope();
    let user = user_label(state, scope, principal.user_id).await?;
    let organization = organizations::get(&state.db, scope)
        .await
        .map_err(|error| ApiError::internal(&error))?;
    let payload = request.pending().to_payload();
    let start = FlowStart {
        kind: FlowKind::OauthAuthorize,
        user_id: Some(principal.user_id),
        provider_id: None,
        payload: &payload,
        return_to: None,
    };
    let started = flow::start(&state.db, &state.keys, scope, start, state.clock.now())
        .await
        .map_err(|error| ApiError::internal(&error))?;
    let mut response = oauth_page::confirmation(request.client.name, &user, &organization.name);
    response
        .headers_mut()
        .append(header::SET_COOKIE, started.set_cookie);
    Ok(response)
}

/// The user as the confirmation page names them: the display name, and the email when there is
/// one.
async fn user_label(state: &AppState, scope: OrgScope, user_id: Uuid) -> Result<String, ApiError> {
    let user = users::get(&state.db, scope, user_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(|| ApiError::internal(&SessionUserMissing))?;
    Ok(match user.email {
        Some(email) => format!("{} ({email})", user.display_name),
        None => user.display_name,
    })
}

#[derive(Debug, thiserror::Error)]
#[error("the session's user is not in the session's organization")]
struct SessionUserMissing;

/// The user's answer on the confirmation page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decision {
    Allow,
    Deny,
}

impl Decision {
    /// The form body's one `decision`, `allow` or `deny`; anything else is `None`.
    fn parse(body: &[u8]) -> Option<Self> {
        let mut decisions = form_urlencoded::parse(body).filter(|(name, _)| name == "decision");
        let decision = match decisions.next()?.1.as_ref() {
            "allow" => Self::Allow,
            "deny" => Self::Deny,
            _ => return None,
        };
        decisions.next().is_none().then_some(decision)
    }
}

/// Answer an app's sign-in from the confirmation page.
#[utoipa::path(
    post,
    path = "/oauth/authorize",
    operation_id = "answerAppAuthorization",
    tag = "auth",
    extensions(("x-stepclimb-mutates" = json!(true))),
    description = "The confirmation page's form: `application/x-www-form-urlencoded` with one \
`decision`, `allow` or `deny`, in the browser that opened the page (the request waits in its \
`__Host-sc_flow` cookie) and signed in as the same user. `allow` is a 303 to the redirect URI \
with `code` (single use, valid 60 seconds) and `state`; `deny` a 303 with \
`error=access_denied` and `state`. Every refusal is an HTML error page: `request.malformed` (no \
single `decision`), `auth.unauthenticated` (401, no session), `auth.flow_invalid` or \
`auth.flow_expired` (the request was answered already, expired, or is another user's).",
    responses(
        (
            status = 303,
            description = "To the redirect URI with `code` and `state`, or with `error=access_denied`; or to `/sign-in?error=auth.rate_limited`",
            headers(
                ("Location" = String, description = "Where the browser goes"),
                ("Set-Cookie" = String, description = "Clears `__Host-sc_flow`"),
                ("Retry-After" = i64, description = "With `auth.rate_limited`: seconds to wait, at least 1")
            )
        ),
        (
            status = 400,
            description = "An HTML error page: `request.malformed`, `auth.flow_invalid` or `auth.flow_expired`",
            content_type = "text/html",
            body = String
        ),
        (
            status = 401,
            description = "An HTML error page: `auth.unauthenticated`",
            content_type = "text/html",
            body = String
        ),
        (status = 403, response = AnswerForbidden),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn answer_app_authorization(
    _limited: NavigationLimited,
    _: NoBearer,
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let decision = match to_bytes(body, MAX_ANSWER_LEN).await {
        Ok(bytes) => Decision::parse(&bytes),
        Err(_) => None,
    };
    let Some(decision) = decision else {
        return Ok(oauth_page::refused(REQUEST_MALFORMED));
    };
    let principal = match signed_in(&state, &headers).await {
        SignedIn::Fault(response) => return Ok(response),
        SignedIn::Yes(principal) => principal,
        SignedIn::No { clear } => {
            let mut page = oauth_page::refused(session::code::AUTH_UNAUTHENTICATED);
            if clear {
                page.headers_mut()
                    .append(header::SET_COOKIE, session::clear_cookie());
            }
            return Ok(page);
        }
    };
    let location = decide(&state, principal, &headers, decision).await?;
    let mut response = match location {
        Ok(location) => to_client(&location),
        Err(code) => oauth_page::refused(code),
    };
    response
        .headers_mut()
        .append(header::SET_COOKIE, flow::clear_cookie());
    Ok(response)
}

/// Take the request's flow and answer it: `Ok` with the redirect URI to send the browser to, or
/// `Err` with the code of the error page.
async fn decide(
    state: &AppState,
    principal: Authenticated,
    headers: &HeaderMap,
    decision: Decision,
) -> Result<Result<String, &'static str>, ApiError> {
    let scope = principal.scope();
    let now = state.clock.now();
    let internal = |error: &dyn std::error::Error| ApiError::internal(error);
    let mut tx = state.db.begin().await.map_err(|e| internal(&e))?;
    let finished = match flow::finish(
        &mut *tx,
        &state.keys,
        scope,
        headers,
        FlowKind::OauthAuthorize,
        now,
    )
    .await
    {
        Ok(finished) => finished,
        Err(error) => {
            let Some(code) = error.code() else {
                return Err(internal(&error));
            };
            // An expired flow's row is deleted by the commit.
            tx.commit().await.map_err(|e| internal(&e))?;
            return Ok(Err(code));
        }
    };
    let request = (finished.user_id == Some(principal.user_id))
        .then(|| Pending::from_payload(&finished.payload))
        .flatten()
        .and_then(|pending| pending.recheck());
    let location = match (request, decision) {
        (None, _) => Err(AUTH_FLOW_INVALID),
        (Some(request), Decision::Deny) => Ok(declined(request)),
        (Some(request), Decision::Allow) => {
            Ok(issue(&mut tx, scope, principal.user_id, &request, now).await?)
        }
    };
    tx.commit().await.map_err(|e| internal(&e))?;
    Ok(location)
}

/// The redirect URI with `error=access_denied`.
fn declined(request: Request) -> String {
    RedirectError {
        redirect_uri: request.redirect_uri,
        error: ErrorCode::AccessDenied,
        description: DECLINED,
        state: request.state,
    }
    .location()
}

/// Store a new code for `request` in the transaction and return the redirect URI with it.
async fn issue(
    tx: &mut sqlx::PgConnection,
    scope: OrgScope,
    user_id: Uuid,
    request: &Request,
    now: std::time::SystemTime,
) -> Result<String, ApiError> {
    let code = Token::generate().map_err(|error| ApiError::internal(&error))?;
    let code_hash = code.hash();
    let new = NewCode {
        id: new_id(),
        user_id,
        client_id: request.client.id,
        code_hash: code_hash.as_bytes(),
        redirect_uri: &request.redirect_uri,
        code_challenge: &request.code_challenge,
    };
    oauth_codes::create(&mut *tx, scope, &new, now)
        .await
        .map_err(|error| ApiError::internal(&error))?;
    let mut pairs = vec![("code", code.expose_secret())];
    if let Some(state) = &request.state {
        pairs.push(("state", state));
    }
    Ok(with_query(&request.redirect_uri, &pairs))
}

#[cfg(test)]
mod tests {
    use stepclimb_core::auth_flows::{MAX_RETURN_TO_LEN, ReturnTo};

    use super::*;

    fn request(state: Option<String>) -> Request {
        let query = format!(
            "response_type=code&client_id=stepclimb-app&redirect_uri=http%3A%2F%2F127.0.0.1%3A65535%2Foauth%2Fcallback\
             &code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM&code_challenge_method=S256{}",
            state
                .map(|s| format!(
                    "&state={}",
                    form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>()
                ))
                .unwrap_or_default()
        );
        authorize::parse(&query).expect("a good request")
    }

    #[test]
    fn the_longest_request_survives_the_sign_in_round_trip() {
        let worst = "%".repeat(authorize::MAX_STATE_LEN);
        let request = request(Some(worst));
        let path = return_to(&request);
        assert!(path.len() <= MAX_RETURN_TO_LEN, "{}", path.len());
        assert_eq!(ReturnTo::sanitize(Some(&path)).as_str(), path);
        let query = path
            .strip_prefix("/oauth/authorize?")
            .expect("the endpoint");
        assert_eq!(authorize::parse(query), Ok(request));
    }

    #[test]
    fn the_sign_in_redirect_carries_the_request_and_can_clear_the_session() {
        let request = request(Some("s 1".into()));
        let response = to_sign_in(&request, true);
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response.headers()[header::LOCATION].to_str().unwrap();
        let query = location
            .strip_prefix("/sign-in?")
            .expect("the sign-in page");
        assert!(
            query.starts_with("return_to=") && !query.contains('&'),
            "{location}"
        );
        let (name, value) = form_urlencoded::parse(query.as_bytes()).next().unwrap();
        assert_eq!(
            (name.as_ref(), value.as_ref()),
            ("return_to", return_to(&request).as_str())
        );
        assert!(
            response.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .starts_with("__Host-sc_session=; Max-Age=0")
        );
        assert!(
            !to_sign_in(&request, false)
                .headers()
                .contains_key(header::SET_COOKIE)
        );
    }

    #[test]
    fn the_answer_is_exactly_one_allow_or_deny() {
        assert_eq!(Decision::parse(b"decision=allow"), Some(Decision::Allow));
        assert_eq!(Decision::parse(b"decision=deny&x=1"), Some(Decision::Deny));
        for body in [
            &b""[..],
            b"decision=",
            b"decision=Allow",
            b"decision=yes",
            b"decision=allow&decision=allow",
            b"decision=deny&decision=allow",
            b"allow",
        ] {
            assert_eq!(
                Decision::parse(body),
                None,
                "{}",
                String::from_utf8_lossy(body)
            );
        }
    }
}
