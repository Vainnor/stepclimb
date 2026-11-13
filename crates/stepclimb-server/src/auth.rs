// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `/auth` routes (docs/specs/m0/08-sessions-and-csrf.md -> API). Today: sign-out.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
};

use crate::{AppState, error::ApiError, session};

/// End the browser's session and clear its cookie.
///
/// Answers 204 whether or not the cookie named a session. It needs no permission and no valid
/// session, so a browser holding an expired or unknown cookie can still clear it.
//
// Spec 08 B13; a request with a bearer header ends no session (B16), see `session::end`.
#[utoipa::path(
    post,
    path = "/auth/sign-out",
    operation_id = "signOut",
    tag = "auth",
    extensions(("x-stepclimb-mutates" = json!(true))),
    responses(
        (
            status = 204,
            description = "The session, if there was one, is ended and the cookie cleared",
            headers(("Set-Cookie" = String, description = "Clears `__Host-sc_session` (`Max-Age=0`)"))
        ),
        (status = 403, response = crate::openapi::CsrfRefused),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn sign_out(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    session::end(&state, &headers).await?;
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, session::clear_cookie())],
    ))
}
