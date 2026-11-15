// SPDX-License-Identifier: AGPL-3.0-or-later
//! Local accounts' passwords: setting one through an enrollment link, signing in with it, and
//! changing it (docs/specs/m0/04-local-accounts.md B5, B9, B14, B16 to B18, Q5, Q8;
//! docs/specs/m0/08-sessions-and-csrf.md B7, B22, B23, B40 to B42).
//!
//! Every route answers 404 `auth.method_disabled` while the organization does not offer
//! passwords (B2), before it reads the body of a sign-in. A new password must pass Q8's policy
//! (`stepclimb_auth::password::check_policy`), or the route answers 422 `request.invalid` with
//! `local.password_weak` at the field, before anything is stored or used up. Hashes are computed
//! by [`PasswordHasher`](crate::password_hasher::PasswordHasher), at most four at once.
//!
//! `POST /auth/local/enrollment/password` ([`enroll_password`], B9) reads the link
//! (`local_tokens::check_enrollment`), hashes the password, then in one transaction locks the
//! link's user, uses the link (`consume_enrollment`, which re-checks it), stores the password
//! (`password.set`), links the local identity if the user has none (`identity.link`), and
//! completes the sign-in through admission's local entry (`admission::complete_local`, which
//! refuses a suspended user with 403 `auth.suspended`). A refusal rolls everything back, so the
//! link stays usable and nothing is stored.
//!
//! `POST /auth/local/password` ([`sign_in_with_password`], B14) finds the login by the email
//! (`local_logins::find_for_sign_in`, which also gives the account identifier the rate limit
//! counts) and checks the password ([`check_password`]). A wrong password, an address with no
//! account (in this organization or at all), and an account without a password all answer the
//! same 401 `auth.credentials_invalid` after one argon2id verification each, the last two
//! against a dummy hash, so neither the body nor the time says which. A right password then, in
//! one transaction, locks the user, refuses the sign-in (401, the same answer) if the password
//! changed since it was read, swaps in a hash at the current parameters when the stored one is
//! older (B16), and completes the sign-in through admission's local entry. It links no
//! identity. Issue I7 inserts B15's TOTP step between the check and the transaction.
//!
//! `PUT /api/v1/me/password` ([`change_my_password`], B17) takes a browser session only
//! (`SessionOnly`), checks the current password the same way, stores the new one
//! (`password.set`) and ends the user's other sessions (Q8), keeping the one that asked.
//!
//! [`check_password`] is B18 and B23: before the verification it takes the account's
//! failed-check budget (`RateLimiter::account_check`, keyed by the login's [`LoginEmail`]),
//! which refuses with 429 `auth.rate_limited` once the account's failures fill the window, and
//! counts the check as a failure until it passes. A browser presenting a device cookie
//! ([`crate::device`]) for the account's user skips that budget (spec 08 B42, the owner's
//! decision); the per-address budget, which every `/auth/` route takes first, still applies.
//! A successful sign-in or enrollment sets a new device cookie.
//!
//! No step logs a password, a hash, the email or the token. Sign-ins write no audit entry (spec
//! 08 Q14); setting a password writes `password.set` with the user.

use std::time::SystemTime;

use axum::{
    Json,
    extract::{FromRequest, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use sqlx::{Postgres, Transaction};
use stepclimb_auth::{
    password::{self as policy, StoredHash, Verified},
    token::TokenHash,
};
use stepclimb_core::{
    OrgScope,
    audit::Actor,
    auth_flows::ReturnTo,
    id::Uuid,
    identities,
    local_logins::{self, Login, LoginEmail},
    local_sign_in::Method,
    local_tokens, sessions,
};
use utoipa::ToSchema;

use crate::{
    AppState,
    admission::{self, Completion},
    audit::ClientAddress,
    device,
    enrollment::{enrollment_error, invalid_link},
    error::{ApiError, FieldError},
    extract::ApiJson,
    local_sign_in::require_method,
    openapi::problem_responses,
    passkey_enrollment::{Outcome, SignedIn, link_local_identity},
    rate_limit::{RateLimited, SignIn, TooManyRequests},
    request_org::RequestOrg,
    require::SessionOnly,
    session,
};

/// The route that signs in with an email and a password.
pub const PASSWORD_SIGN_IN_PATH: &str = "/auth/local/password";

/// The route that sets a password through an enrollment link.
pub const ENROLLMENT_PASSWORD_PATH: &str = "/auth/local/enrollment/password";

/// The route that changes the signed-in user's password.
pub const MY_PASSWORD_PATH: &str = "/api/v1/me/password";

/// The codes this module produces.
pub mod code {
    /// The email and password did not sign in, or the current password is wrong (401).
    pub const AUTH_CREDENTIALS_INVALID: &str = "auth.credentials_invalid";
    /// In a 422's `errors`: the new password breaks Q8's policy.
    pub const LOCAL_PASSWORD_WEAK: &str = "local.password_weak";
}

problem_responses! {
    PasswordSignInUnauthorized => "`auth.credentials_invalid`: the email and password do not match an account of this organization that has a password.",
    PasswordSignInForbidden => "`auth.suspended`: the account is suspended. `auth.csrf_refused`: the request did not come from this site.",
    PasswordNotFound => "`auth.method_disabled`: password sign-in is switched off for this organization. `org.not_found`: the request's organization cannot be determined.",
    EnrollPasswordInvalid => "`request.malformed`: the body is not `{token, password}`. `auth.enrollment_invalid`: the link is unknown, already used, replaced by a newer link, or for another organization. `auth.enrollment_expired`: the link is older than its 7-day lifetime.",
    PasswordWeak => "`request.invalid` with `local.password_weak` at the password: it has fewer than 12 or more than 256 characters, or it is one of the 10,000 most common passwords.",
    MyPasswordUnauthorized => "`auth.unauthenticated`, `auth.session_expired`, `auth.suspended`: no valid session. `auth.credentials_invalid`: the current password is wrong.",
    MyPasswordForbidden => "`auth.session_required`: the request carries a bearer credential; only a signed-in browser session changes a password. `auth.csrf_refused`: the request did not come from this site.",
}

/// An email and password to sign in with. Never logged.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PasswordSignIn {
    /// The account's email, in any letter case.
    pub email: String,
    pub password: String,
    /// The path on this site to open after signing in; anything else becomes `/`.
    pub return_to: Option<String>,
}

/// The password to set through an enrollment link. Never logged.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PasswordEnrollment {
    /// The text after `#token=` in the link.
    pub token: String,
    /// 12 to 256 characters, not one of the most common passwords.
    pub password: String,
}

/// The signed-in user's current password and the new one. Never logged.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PasswordChange {
    pub current_password: String,
    /// 12 to 256 characters, not one of the most common passwords.
    pub new_password: String,
}

/// Set a password through an enrollment link and sign in with it.
#[utoipa::path(
    post,
    path = "/auth/local/enrollment/password",
    operation_id = "enrollPassword",
    tag = "auth",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = PasswordEnrollment,
    responses(
        (
            status = 200,
            description = "The password is set and the user is signed in",
            body = SignedIn,
            headers(
                ("Cache-Control" = String, description = "`no-store`"),
                ("Set-Cookie" = String, description = "The new `__Host-sc_session` cookie and the `__Host-sc_device` cookie")
            )
        ),
        (status = 400, response = EnrollPasswordInvalid),
        (status = 403, response = PasswordSignInForbidden),
        (status = 404, response = PasswordNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = PasswordWeak),
        (status = 429, response = crate::openapi::RateLimited),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn enroll_password(
    _limited: RateLimited<SignIn>,
    org: RequestOrg,
    client: ClientAddress,
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<PasswordEnrollment>,
) -> Result<Response, ApiError> {
    let scope = org.scope();
    require_method(&state.db, scope, Method::Password).await?;
    refuse_weak(&body.password, "password")?;
    let now = state.clock.now();
    let token_hash = TokenHash::of(&body.token);
    let mut conn = state.db.acquire().await.map_err(internal)?;
    let link = local_tokens::check_enrollment(&mut conn, scope, token_hash.as_bytes(), now)
        .await
        .map_err(enrollment_error)?;
    drop(conn);
    let stored = state
        .passwords
        .hash(body.password)
        .await
        .map_err(internal)?;
    let mut tx = state.db.begin().await.map_err(internal)?;
    let user_id = link.user_id;
    let actor = Actor::User {
        id: user_id,
        ip: client.0,
    };
    use_link(&mut tx, scope, user_id, &token_hash, now).await?;
    local_logins::set_password(&mut *tx, scope, user_id, stored.as_str(), now, actor)
        .await
        .map_err(internal)?;
    link_local_identity(&mut tx, scope, user_id, now, actor)
        .await
        .map_err(|outcome| match outcome {
            Outcome::Refused(error) | Outcome::Failed(error) => error,
        })?;
    let started = complete(&mut tx, scope, user_id, now, &headers).await?;
    tx.commit().await.map_err(internal)?;
    tracing::info!(%user_id, "password set through an enrollment link");
    signed_in(
        &state,
        scope,
        user_id,
        started,
        ReturnTo::root().as_str(),
        now,
    )
}

/// Lock the link's user, then use the link (the order `create_enrollment` takes them in), and
/// refuse a link that no longer names that user.
async fn use_link(
    tx: &mut Transaction<'_, Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    token_hash: &TokenHash,
    now: SystemTime,
) -> Result<(), ApiError> {
    match identities::lock_user(tx, scope, user_id).await {
        Ok(_) => {}
        Err(identities::Error::UserNotFound) => return Err(invalid_link()),
        Err(other) => return Err(internal(other)),
    }
    let used = local_tokens::consume_enrollment(tx, scope, token_hash.as_bytes(), now).await;
    match used {
        Ok(enrollment) if enrollment.user_id == user_id => Ok(()),
        Ok(_) => Err(invalid_link()),
        Err(error @ (local_tokens::Error::Invalid | local_tokens::Error::Expired)) => {
            Err(enrollment_error(error))
        }
        Err(other) => Err(internal(other)),
    }
}

/// Sign in with an email and a password.
#[utoipa::path(
    post,
    path = "/auth/local/password",
    operation_id = "signInWithPassword",
    tag = "auth",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = PasswordSignIn,
    responses(
        (
            status = 200,
            description = "The user is signed in",
            body = SignedIn,
            headers(
                ("Cache-Control" = String, description = "`no-store`"),
                ("Set-Cookie" = String, description = "The new `__Host-sc_session` cookie and the `__Host-sc_device` cookie")
            )
        ),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = PasswordSignInUnauthorized),
        (status = 403, response = PasswordSignInForbidden),
        (status = 404, response = PasswordNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 429, response = crate::openapi::RateLimited),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn sign_in_with_password(
    _limited: RateLimited<SignIn>,
    org: RequestOrg,
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
) -> Result<Response, Refusal> {
    let scope = org.scope();
    require_method(&state.db, scope, Method::Password).await?;
    let ApiJson(body) = ApiJson::<PasswordSignIn>::from_request(request, &state).await?;
    let (email, login) = local_logins::find_for_sign_in(&state.db, scope, &body.email)
        .await
        .map_err(internal)?;
    let now = state.clock.now();
    let attempt = Attempt {
        state: &state,
        scope,
        headers: &headers,
        now,
    };
    let passed = check_password(&attempt, Some(&email), login.as_ref(), &body.password).await?;
    let started = finish_sign_in(&attempt, &passed).await?;
    tracing::info!(user_id = %passed.user_id, "password sign-in");
    let return_to = ReturnTo::sanitize(body.return_to.as_deref());
    signed_in(
        &state,
        scope,
        passed.user_id,
        started,
        return_to.as_str(),
        now,
    )
    .map_err(Refusal::from)
}

/// In one transaction: lock the user, refuse the sign-in if the password changed since
/// [`check_password`] read it, store the rehash if there is one, and complete the sign-in.
async fn finish_sign_in(
    attempt: &Attempt<'_>,
    passed: &Passed,
) -> Result<session::Started, ApiError> {
    let scope = attempt.scope;
    let mut tx = attempt.state.db.begin().await.map_err(internal)?;
    identities::lock_user(&mut tx, scope, passed.user_id)
        .await
        .map_err(internal)?;
    let current = local_logins::get(&mut *tx, scope, passed.user_id)
        .await
        .map_err(internal)?
        .and_then(|login| login.password_hash);
    if current.as_deref() != Some(passed.verified.as_str()) {
        tracing::info!(user_id = %passed.user_id, "password sign-in refused: the password changed during the check");
        return Err(credentials_invalid());
    }
    if let Some(rehashed) = &passed.rehash {
        let (old, new) = (passed.verified.as_str(), rehashed.as_str());
        local_logins::replace_hash(&mut tx, scope, passed.user_id, old, new, attempt.now)
            .await
            .map_err(internal)?;
    }
    let started = complete(&mut tx, scope, passed.user_id, attempt.now, attempt.headers).await?;
    tx.commit().await.map_err(internal)?;
    Ok(started)
}

/// Change the signed-in user's password.
#[utoipa::path(
    put,
    path = "/api/v1/me/password",
    operation_id = "changeMyPassword",
    tag = "me",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = PasswordChange,
    responses(
        (status = 204, description = "The password is changed and every other session of the user has ended"),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = MyPasswordUnauthorized),
        (status = 403, response = MyPasswordForbidden),
        (status = 404, response = PasswordNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = PasswordWeak),
        (status = 429, response = crate::openapi::RateLimited),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn change_my_password(
    session: SessionOnly,
    client: ClientAddress,
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<PasswordChange>,
) -> Result<Response, Refusal> {
    let principal = session.principal();
    let scope = principal.scope();
    require_method(&state.db, scope, Method::Password).await?;
    refuse_weak(&body.new_password, "new_password")?;
    let login = local_logins::get(&state.db, scope, principal.user_id)
        .await
        .map_err(internal)?;
    let email = login.as_ref().and_then(|login| login.email.as_ref());
    let attempt = Attempt {
        state: &state,
        scope,
        headers: &headers,
        now: state.clock.now(),
    };
    check_password(&attempt, email, login.as_ref(), &body.current_password).await?;
    let stored = state
        .passwords
        .hash(body.new_password)
        .await
        .map_err(internal)?;
    let actor = client.actor(principal);
    store_change(&attempt, principal.user_id, &stored, actor).await?;
    tracing::info!(user_id = %principal.user_id, "password changed");
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Store the changed password and end the user's other sessions, under the user's lock.
async fn store_change(
    attempt: &Attempt<'_>,
    user_id: Uuid,
    stored: &StoredHash,
    actor: Actor,
) -> Result<(), ApiError> {
    let scope = attempt.scope;
    // `SessionOnly` let the request in, so it presents exactly one session cookie.
    let keep = session::presented_hash(attempt.headers).ok_or_else(|| {
        internal(std::io::Error::other(
            "a session-only request presents no single session cookie",
        ))
    })?;
    let mut tx = attempt.state.db.begin().await.map_err(internal)?;
    identities::lock_user(&mut tx, scope, user_id)
        .await
        .map_err(internal)?;
    local_logins::set_password(
        &mut *tx,
        scope,
        user_id,
        stored.as_str(),
        attempt.now,
        actor,
    )
    .await
    .map_err(internal)?;
    sessions::delete_others_of_user(&mut *tx, scope, user_id, keep.as_bytes())
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(())
}

/// Why a password route refused: problem details, or the account's budget spent (429 with
/// `Retry-After`).
pub enum Refusal {
    Problem(ApiError),
    Limited(TooManyRequests),
}

impl From<ApiError> for Refusal {
    fn from(error: ApiError) -> Self {
        Self::Problem(error)
    }
}

impl From<TooManyRequests> for Refusal {
    fn from(limited: TooManyRequests) -> Self {
        Self::Limited(limited)
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        match self {
            Self::Problem(error) => error.into_response(),
            Self::Limited(limited) => limited.into_response(),
        }
    }
}

/// What a password check needs besides the password.
struct Attempt<'a> {
    state: &'a AppState,
    scope: OrgScope,
    headers: &'a HeaderMap,
    now: SystemTime,
}

/// A password that matched: the user, the hash it matched, and the hash at the current
/// parameters to store instead when that one is older.
struct Passed {
    user_id: Uuid,
    verified: StoredHash,
    rehash: Option<StoredHash>,
}

/// Check `password` against `login`'s hash, or the dummy hash when there is none (B14, B18):
/// one argon2id verification whatever the case, under the account's failed-check budget unless
/// the browser's device cookie names the login's user. A refusal is 401
/// `auth.credentials_invalid`, 429 `auth.rate_limited`, or a fault.
async fn check_password(
    attempt: &Attempt<'_>,
    email: Option<&LoginEmail>,
    login: Option<&Login>,
    password: &str,
) -> Result<Passed, Refusal> {
    let state = attempt.state;
    let device_user = device::user(&state.keys, attempt.scope, attempt.headers, attempt.now);
    let exempt = login.is_some_and(|login| device_user == Some(login.user_id));
    let budget = match email {
        Some(email) if !exempt => Some(state.rate_limits.account_check(
            attempt.scope,
            email,
            attempt.now,
        )?),
        _ => None,
    };
    let stored = login
        .and_then(|login| login.password_hash.clone())
        .map(StoredHash::new);
    let verified = state
        .passwords
        .verify(password.to_owned(), stored.clone())
        .await
        .map_err(internal)?;
    let (Verified::Match { rehash }, Some(login), Some(stored)) = (verified, login, stored) else {
        // The budget, when taken, keeps this failure counted.
        drop(budget);
        tracing::info!(user_id = ?login.map(|login| login.user_id), "password check failed");
        return Err(credentials_invalid().into());
    };
    if let Some(budget) = budget {
        budget.passed();
    }
    let rehash = if rehash {
        let new = state.passwords.hash(password.to_owned()).await;
        Some(new.map_err(internal)?)
    } else {
        None
    };
    Ok(Passed {
        user_id: login.user_id,
        verified: stored,
        rehash,
    })
}

/// Admission's local entry (spec 07 B1, step 1): the session, or the refusal's 403.
async fn complete(
    tx: &mut Transaction<'_, Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    now: SystemTime,
    headers: &HeaderMap,
) -> Result<session::Started, ApiError> {
    match admission::complete_local(tx, scope, user_id, now, headers).await {
        Ok(started) => Ok(started),
        Err(Completion::Refused(refusal)) => Err(refusal.into_error()),
        Err(Completion::Failed(error)) => Err(error),
    }
}

/// 422 `request.invalid` with `local.password_weak` at `field` unless `password` passes Q8.
fn refuse_weak(password: &str, field: &str) -> Result<(), ApiError> {
    policy::check_policy(password)
        .map_err(|_| ApiError::invalid(vec![FieldError::at(&[field], code::LOCAL_PASSWORD_WEAK)]))
}

/// 200 `{return_to}` with the session cookie, a new device cookie for `user_id` and
/// `no-store`.
fn signed_in(
    state: &AppState,
    scope: OrgScope,
    user_id: Uuid,
    started: session::Started,
    return_to: &str,
    now: SystemTime,
) -> Result<Response, ApiError> {
    let device = device::set_cookie(&state.keys, scope, user_id, now).map_err(internal)?;
    let body = SignedIn {
        return_to: return_to.to_owned(),
    };
    let mut response = Json(body).into_response();
    let headers = response.headers_mut();
    headers.append(header::SET_COOKIE, started.set_cookie);
    headers.append(header::SET_COOKIE, device);
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

fn credentials_invalid() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        code::AUTH_CREDENTIALS_INVALID,
        "The email or password is wrong. Check both and try again.",
    )
}

fn internal<E: std::error::Error>(error: E) -> ApiError {
    ApiError::internal(&error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_weak_password_is_422_at_its_field() {
        let problem = refuse_weak("short", "new_password").unwrap_err().problem();
        assert_eq!(
            (problem.status, problem.code.as_str()),
            (422, "request.invalid")
        );
        assert_eq!(problem.errors[0].field, "/new_password");
        assert_eq!(problem.errors[0].code, "local.password_weak");
        assert!(refuse_weak("correct horse battery staple", "password").is_ok());
    }

    #[test]
    fn wrong_credentials_are_401() {
        let problem = credentials_invalid().problem();
        assert_eq!(
            (problem.status, problem.code.as_str()),
            (401, "auth.credentials_invalid")
        );
    }
}
