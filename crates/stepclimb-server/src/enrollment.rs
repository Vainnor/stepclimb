// SPDX-License-Identifier: AGPL-3.0-or-later
//! Enrollment links (docs/specs/m0/04-local-accounts.md B6, B7, B10, Q6, Q7).
//!
//! An enrollment link is a one-time link to the web app that lets one user set up a local
//! credential (a passkey or a password) without signing in. Until the server can send email
//! (M1) an admin creates the link and hands it over. The link is the public URL's
//! [`ENROLL_PAGE`] with the token in the fragment (`#token=...`): a browser sends no fragment to
//! any server, so the token stays out of access logs and `Referer` headers, and the web page
//! posts it to [`ENROLLMENT_START_PATH`].
//!
//! [`create_link`] is the one function that makes a link: the admin route below and
//! `stepclimb admin create` (Q7) call it. It generates a 256-bit token, stores only its SHA-256
//! with an expiry 7 days later (`local_tokens::create_enrollment`, which also removes the user's
//! earlier unused links and writes the `enrollment_link.create` audit entry), and returns the
//! link once. The token is never logged: [`SetupLink`]'s `Debug` leaves it out.
//!
//! `POST /api/v1/admin/users/{user_id}/enrollment-links` needs `user.manage` at organization
//! scope. A link lets whoever holds it set up a way to sign in as the user, so it applies the
//! user routes' escalation rule (`users::refuse_escalation`, 403 `permission.escalation`) under
//! the locks the identity admin routes take (`identities::begin_on_user`). A user without an
//! email answers 409 `local.email_required`, since local sign-in finds a user by email (B4).
//!
//! `POST /auth/local/enrollment/start` needs no credential. It is rate limited per client
//! address first (`RateLimited<SignIn>`), then answers 404 `auth.method_disabled` unless the
//! organization offers a method a link can set up (the passkey or the password, B2). A token
//! that no unused link of the request's organization has answers 400 `auth.enrollment_invalid`,
//! one past its expiry 400 `auth.enrollment_expired` (B7). It uses nothing up: the passkey and
//! password routes of issues I3 and I6 mark the link used when they store the credential, and
//! the passkey options and their sign-in flow arrive with issue I3.

use std::time::SystemTime;

use axum::{
    Json,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::{Acquire, Postgres};
use stepclimb_auth::token::{RandomError, Token, TokenHash};
use stepclimb_core::{
    OrgScope, audit::Actor, id::Uuid, local_sign_in::Method, local_tokens as repo, perm::UserManage,
};
use utoipa::ToSchema;

use crate::{
    AppState,
    audit::ClientAddress,
    error::ApiError,
    extract::{ApiJson, ApiPath},
    identities::begin_on_user,
    local_sign_in::require_any_method,
    openapi::problem_responses,
    rate_limit::{RateLimited, SignIn},
    request_org::RequestOrg,
    require::Require,
    timestamp,
    users::{UserIdParam, user_not_found},
    web_origin::WebOrigin,
};

/// The web app's page an enrollment link opens.
pub const ENROLL_PAGE: &str = "/enroll";

/// The route the enrollment page posts the token to.
pub const ENROLLMENT_START_PATH: &str = "/auth/local/enrollment/start";

/// The codes this module produces.
pub mod code {
    /// No unused enrollment link of the organization has this token (400).
    pub const AUTH_ENROLLMENT_INVALID: &str = "auth.enrollment_invalid";
    /// The enrollment link passed its lifetime (400).
    pub const AUTH_ENROLLMENT_EXPIRED: &str = "auth.enrollment_expired";
    /// The user has no email, which local sign-in finds them by (409).
    pub const LOCAL_EMAIL_REQUIRED: &str = "local.email_required";
}

/// The local methods an enrollment link can set up, in the order the API lists them. TOTP is
/// set up after a password, signed in; the magic link needs no setup.
const ENROLLABLE: [Method; 2] = [Method::Passkey, Method::Password];

problem_responses! {
    EnrollmentLinkConflict => "`local.email_required`: the user has no email address, which local sign-in finds them by; add one first.",
    EnrollmentStartInvalid => "`request.malformed`: the body is not `{token}`. `auth.enrollment_invalid`: the link is unknown, already used, replaced by a newer link, or for another organization. `auth.enrollment_expired`: the link is older than its 7-day lifetime; ask an admin for a new one.",
    EnrollmentStartNotFound => "`auth.method_disabled`: the organization offers neither passkey nor password sign-in. `org.not_found`: the request's organization cannot be determined.",
}

/// An enrollment link, returned once: the only copy of its token.
#[derive(Serialize, ToSchema)]
pub struct EnrollmentLink {
    /// The link to hand to the user. It holds a secret: anyone with it can set up a way to
    /// sign in as the user until it is used, replaced or expires.
    pub url: String,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub expires_at: String,
}

/// A link [`create_link`] made. `Debug` leaves the URL out, since it holds the token.
#[derive(Clone, PartialEq, Eq)]
pub struct SetupLink {
    /// The link's row id, which its audit entry names.
    pub id: Uuid,
    url: String,
    pub expires_at: SystemTime,
    /// `expires_at` as the API writes it (RFC 3339, UTC).
    pub expires_at_text: String,
}

impl SetupLink {
    /// The link, for the one response or the one line of output that hands it over. Never log
    /// it.
    pub fn expose_url(&self) -> &str {
        &self.url
    }
}

impl std::fmt::Debug for SetupLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetupLink")
            .field("id", &self.id)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// Why [`create_link`] failed.
#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error(transparent)]
    Random(#[from] RandomError),
    #[error(transparent)]
    Store(#[from] repo::Error),
    #[error(transparent)]
    Time(#[from] timestamp::Unrepresentable),
}

/// Make an enrollment link for the user `user_id` of the scope's organization at `now`, by
/// `actor` (`created_by` is the admin, `None` for the instance), opening `public_url`'s
/// [`ENROLL_PAGE`]. Runs in one transaction (a savepoint when `db` is already one).
pub async fn create_link<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    created_by: Option<Uuid>,
    public_url: &WebOrigin,
    now: SystemTime,
    actor: Actor,
) -> Result<SetupLink, LinkError> {
    let token = Token::generate()?;
    let hash = token.hash();
    let stored =
        repo::create_enrollment(db, scope, user_id, hash.as_bytes(), created_by, now, actor)
            .await?;
    let expires_at_text = timestamp::rfc3339(stored.expires_at)?;
    Ok(SetupLink {
        id: stored.id,
        expires_at_text,
        url: format!("{public_url}{ENROLL_PAGE}#token={}", token.expose_secret()),
        expires_at: stored.expires_at,
    })
}

/// Create an enrollment link for a user, replacing their earlier unused one.
#[utoipa::path(
    post,
    path = "/api/v1/admin/users/{user_id}/enrollment-links",
    operation_id = "createEnrollmentLink",
    tag = "users",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(UserIdParam),
    responses(
        (status = 201, description = "The link, shown once", body = EnrollmentLink),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::identities::IdentityWriteForbidden),
        (status = 404, response = crate::users::UserNotFound),
        (status = 409, response = EnrollmentLinkConflict),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn create_enrollment_link(
    require: Require<UserManage>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiPath(UserIdParam { user_id }): ApiPath<UserIdParam>,
) -> Result<Response, ApiError> {
    let principal = require.principal();
    let scope = principal.scope();
    let mut tx = begin_on_user(&state, require.grants(), user_id).await?;
    let actor = client.actor(principal);
    let now = state.clock.now();
    let admin = Some(principal.user_id);
    let link = create_link(
        &mut *tx,
        scope,
        user_id,
        admin,
        &state.public_url,
        now,
        actor,
    )
    .await
    .map_err(link_error)?;
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    let body = EnrollmentLink {
        url: link.url,
        expires_at: link.expires_at_text,
    };
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

fn link_error(error: LinkError) -> ApiError {
    match error {
        LinkError::Store(repo::Error::UserNotFound) => user_not_found(),
        LinkError::Store(repo::Error::EmailMissing) => ApiError::new(
            StatusCode::CONFLICT,
            code::LOCAL_EMAIL_REQUIRED,
            "This user has no email address, which local sign-in finds them by. Add one first.",
        ),
        other => ApiError::internal(&other),
    }
}

/// The token from an enrollment link.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentStart {
    /// The text after `#token=` in the link.
    pub token: String,
}

/// A local method an enrollment link can set up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EnrollMethod {
    Passkey,
    Password,
}

/// What an enrollment link sets up.
#[derive(Debug, Serialize, ToSchema)]
pub struct EnrollmentStarted {
    /// The email the user will sign in with.
    pub email: String,
    /// The methods the organization lets the user set up, passkey first. Never empty.
    pub methods: Vec<EnrollMethod>,
}

/// Open an enrollment link: say whose it is and what it can set up, without using it.
#[utoipa::path(
    post,
    path = "/auth/local/enrollment/start",
    operation_id = "startEnrollment",
    tag = "auth",
    request_body = EnrollmentStart,
    responses(
        (
            status = 200,
            description = "The link is live",
            body = EnrollmentStarted,
            headers(("Cache-Control" = String, description = "`no-store`: the answer holds the user's email"))
        ),
        (status = 400, response = EnrollmentStartInvalid),
        (status = 403, response = crate::openapi::CsrfRefused),
        (status = 404, response = EnrollmentStartNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 429, response = crate::openapi::RateLimited),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn start_enrollment(
    _limited: RateLimited<SignIn>,
    org: RequestOrg,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<EnrollmentStart>,
) -> Result<Response, ApiError> {
    let scope = org.scope();
    let methods = enrollable_methods(&state, scope).await?;
    let hash = TokenHash::of(&body.token);
    let enrollment = repo::check_enrollment(&state.db, scope, hash.as_bytes(), state.clock.now())
        .await
        .map_err(start_error)?;
    let started = EnrollmentStarted {
        email: enrollment.email,
        methods,
    };
    let no_store = [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))];
    Ok((no_store, Json(started)).into_response())
}

/// The methods of [`ENROLLABLE`] the organization offers, or 404 `auth.method_disabled` when it
/// offers none (B2), read before the token.
async fn enrollable_methods(
    state: &AppState,
    scope: OrgScope,
) -> Result<Vec<EnrollMethod>, ApiError> {
    let offered = require_any_method(&state.db, scope, &ENROLLABLE).await?;
    Ok(offered
        .into_iter()
        .filter_map(|method| match method {
            Method::Passkey => Some(EnrollMethod::Passkey),
            Method::Password => Some(EnrollMethod::Password),
            Method::Totp | Method::MagicLink => None,
        })
        .collect())
}

fn start_error(error: repo::Error) -> ApiError {
    match error {
        repo::Error::Invalid => ApiError::new(
            StatusCode::BAD_REQUEST,
            code::AUTH_ENROLLMENT_INVALID,
            "This enrollment link is not valid. It may have been used or replaced; ask an admin for a new one.",
        ),
        repo::Error::Expired => ApiError::new(
            StatusCode::BAD_REQUEST,
            code::AUTH_ENROLLMENT_EXPIRED,
            "This enrollment link has expired. Ask an admin for a new one.",
        ),
        other => ApiError::internal(&other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setup_links_debug_output_leaves_the_token_out() {
        let link = SetupLink {
            id: Uuid::nil(),
            url: "https://lms.example.org/enroll#token=secret-token-text".into(),
            expires_at: SystemTime::UNIX_EPOCH,
            expires_at_text: "1970-01-01T00:00:00.000000Z".into(),
        };
        let debug = format!("{link:?}");
        assert!(!debug.contains("secret-token-text"), "{debug}");
        assert!(!debug.contains("enroll"), "{debug}");
    }

    #[test]
    fn repository_refusals_map_to_their_codes() {
        let invalid = start_error(repo::Error::Invalid).problem();
        assert_eq!(
            (invalid.status, invalid.code.as_str()),
            (400, "auth.enrollment_invalid")
        );
        let expired = start_error(repo::Error::Expired).problem();
        assert_eq!(
            (expired.status, expired.code.as_str()),
            (400, "auth.enrollment_expired")
        );
        let email = link_error(LinkError::Store(repo::Error::EmailMissing)).problem();
        assert_eq!(
            (email.status, email.code.as_str()),
            (409, "local.email_required")
        );
        let user = link_error(LinkError::Store(repo::Error::UserNotFound)).problem();
        assert_eq!((user.status, user.code.as_str()), (404, "user.not_found"));
    }
}
