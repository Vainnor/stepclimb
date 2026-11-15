// SPDX-License-Identifier: AGPL-3.0-or-later
//! Signing in with a passkey, email first (docs/specs/m0/04-local-accounts.md B5, B11, B12, Q9,
//! Q12; docs/specs/m0/08-sessions-and-csrf.md B7, B17 to B20).
//!
//! The ceremony takes two requests, each rate limited first and answering 404
//! `auth.method_disabled` while passkeys are off.
//!
//! `POST /auth/local/passkey/start` ([`start_passkey_sign_in`]) finds the user by the
//! organization and the email (spec 04 B4) and answers credential request options that allow
//! that user's passkeys, bound to a new `passkey_sign_in` flow whose sealed payload holds the
//! ceremony state. An address with no account, another organization's, or one whose account
//! has no passkey gets the same answer with no credential allowed (Q9 (a)), and a flow that no
//! assertion can finish.
//!
//! `POST /auth/local/passkey/finish` ([`finish_passkey_sign_in`]) takes the flow (deleting it,
//! so the challenge is used once), then, in a savepoint:
//!
//! 1. refuses a flow started for no account;
//! 2. locks the passkey the assertion names by `(org_id, credential_id)`, which must belong to
//!    the flow's user;
//! 3. verifies the assertion (`stepclimb_auth::passkey`: challenge, exact origin, relying party
//!    ID, user verification, signature, user handle, sign counter against the stored one);
//! 4. admits the user: spec 07's pipeline is not built, and for a known user its only step is
//!    step 1, which refuses a suspended user with 403 `auth.suspended`;
//! 5. saves the credential's new counter and backup state and the time it was used;
//! 6. creates the session (spec 08 B7), ending the sessions the browser's old cookies name.
//!
//! Every refusal at steps 1 to 3 is the same 401 `auth.passkey_invalid`, so the answer does not
//! say whether the account, the passkey or the signature was wrong. A refusal rolls the
//! savepoint back and commits the flow's deletion; every answer after the flow is taken clears
//! the flow cookie. No step logs the flow token, the ceremony state or the assertion.
//! A sign-in writes no audit entry (spec 08 Q14: routine sign-ins are logged only).

use std::time::SystemTime;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Acquire, PgConnection, Postgres, Transaction};
use stepclimb_auth::passkey::{self as webauthn, Assertion};
use stepclimb_core::{
    OrgScope,
    auth_flows::Kind,
    id::Uuid,
    local_sign_in::Method,
    passkeys,
    users::{self, Status, User},
};
use utoipa::ToSchema;

use crate::{
    AppState,
    error::ApiError,
    extract::ApiJson,
    flow::{self, FlowStart},
    local_sign_in::require_method,
    openapi::problem_responses,
    passkey_enrollment::{
        FlowDetails, Outcome, SignedIn, flow_refused, relying_party, signed_in, suspended,
        with_flow_cleared,
    },
    passkey_options::PasskeyRequestOptions,
    rate_limit::{RateLimited, SignIn},
    request_org::RequestOrg,
    session,
};

/// The route that starts the ceremony.
pub const PASSKEY_START_PATH: &str = "/auth/local/passkey/start";

/// The route that finishes it.
pub const PASSKEY_FINISH_PATH: &str = "/auth/local/passkey/finish";

/// The codes this module produces.
pub mod code {
    /// The assertion did not sign in: no account or passkey matched, or it failed
    /// verification (401).
    pub const AUTH_PASSKEY_INVALID: &str = "auth.passkey_invalid";
}

problem_responses! {
    PasskeySignInStartNotFound => "`auth.method_disabled`: passkey sign-in is switched off for this organization. `org.not_found`: the request's organization cannot be determined.",
    PasskeySignInFinishBadRequest => "`request.malformed`: the body is not `{credential}`. `auth.flow_invalid`: no passkey sign-in was started in this browser, or it was already finished; start again. `auth.flow_expired`: the sign-in was started more than 10 minutes ago; start again.",
    PasskeySignInFinishUnauthorized => "`auth.passkey_invalid`: the passkey did not sign in: it is not a passkey of the account the sign-in was started for, or its answer did not verify. Start again.",
    PasskeySignInFinishForbidden => "`auth.suspended`: the account is suspended. `auth.csrf_refused`: the request did not come from this site.",
    PasskeySignInFinishNotFound => "`auth.method_disabled`: passkey sign-in is switched off for this organization. `org.not_found`: the request's organization cannot be determined.",
}

const SIGN_IN_FLOW: FlowDetails = FlowDetails {
    expired: "This passkey sign-in started more than 10 minutes ago. Start again.",
    invalid: "No passkey sign-in is waiting in this browser. Start again.",
};

/// What the start route's flow keeps, sealed: the ceremony state, with the challenge and the
/// credentials it allows. Never logged.
#[derive(Serialize, Deserialize)]
struct Pending {
    state: String,
}

/// The address to sign in with.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PasskeySignInStart {
    /// The account's email, in any letter case.
    pub email: String,
    /// The path on this site to open after signing in; anything else becomes `/`.
    pub return_to: Option<String>,
}

/// The request options for the browser.
#[derive(Debug, Serialize, ToSchema)]
pub struct PasskeySignInStarted {
    /// Pass to `navigator.credentials.get`, then send the result to `POST
    /// /auth/local/passkey/finish` in this browser within 10 minutes.
    pub options: PasskeyRequestOptions,
}

/// Start signing in with a passkey.
#[utoipa::path(
    post,
    path = "/auth/local/passkey/start",
    operation_id = "startPasskeySignIn",
    tag = "auth",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = PasskeySignInStart,
    responses(
        (
            status = 200,
            description = "The request options, whether or not the address has an account or a passkey",
            body = PasskeySignInStarted,
            headers(
                ("Cache-Control" = String, description = "`no-store`"),
                ("Set-Cookie" = String, description = "The `__Host-sc_flow` cookie of the sign-in")
            )
        ),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 403, response = crate::openapi::CsrfRefused),
        (status = 404, response = PasskeySignInStartNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 429, response = crate::openapi::RateLimited),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn start_passkey_sign_in(
    _limited: RateLimited<SignIn>,
    org: RequestOrg,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<PasskeySignInStart>,
) -> Result<Response, ApiError> {
    let scope = org.scope();
    require_method(&state.db, scope, Method::Passkey).await?;
    let now = state.clock.now();
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let begun = begin(&state, &mut conn, scope, &body, now).await?;
    let mut response = Json(PasskeySignInStarted {
        options: begun.options,
    })
    .into_response();
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.append(header::SET_COOKIE, begun.set_cookie);
    Ok(response)
}

/// A started sign-in: the request options for the browser, and the flow cookie that holds the
/// ceremony on the server.
pub struct Begun {
    pub options: PasskeyRequestOptions,
    /// The `Set-Cookie` value. It holds the flow token: never log it.
    pub set_cookie: HeaderValue,
}

/// Start a passkey sign-in for `request` at `now` (B11, Q9 (a)): request options allowing the
/// passkeys of the scope's user with that email, none when there is no such user, and the
/// `passkey_sign_in` flow that keeps the ceremony state.
pub async fn begin(
    state: &AppState,
    conn: &mut PgConnection,
    scope: OrgScope,
    request: &PasskeySignInStart,
    now: SystemTime,
) -> Result<Begun, ApiError> {
    let user = account(&mut *conn, scope, &request.email).await?;
    let credentials = match &user {
        Some(user) => passkeys::credentials(&mut *conn, scope, user.id)
            .await
            .map_err(|e| ApiError::internal(&e))?,
        None => Vec::new(),
    };
    let party = relying_party(state, &mut *conn, scope).await?;
    let started = party
        .start_authentication(&credentials)
        .map_err(|e| ApiError::internal(&e))?;
    let options =
        PasskeyRequestOptions::from_json(started.options).map_err(|e| ApiError::internal(&e))?;
    let payload = serde_json::to_vec(&Pending {
        state: started.state,
    })
    .map_err(|e| ApiError::internal(&e))?;
    let flow = FlowStart {
        kind: Kind::PasskeySignIn,
        user_id: user.map(|user| user.id),
        payload: &payload,
        return_to: request.return_to.as_deref(),
    };
    let flow = flow::start(&mut *conn, &state.keys, scope, flow, now)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    Ok(Begun {
        options,
        set_cookie: flow.set_cookie,
    })
}

/// The user of the scope's organization with `email`, or `None`. An address that cannot be
/// stored (it holds NUL) has no account either, and is answered the same way.
async fn account(
    conn: &mut PgConnection,
    scope: OrgScope,
    email: &str,
) -> Result<Option<User>, ApiError> {
    match users::find_by_email(&mut *conn, scope, email).await {
        Ok(user) => Ok(user),
        Err(users::Error::ContainsNul { .. }) => Ok(None),
        Err(other) => Err(ApiError::internal(&other)),
    }
}

/// The browser's answer to the request options.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PasskeySignInFinish {
    /// The `PublicKeyCredential` `navigator.credentials.get` returned, as JSON
    /// (`PublicKeyCredential.toJSON()`).
    #[schema(value_type = Object)]
    pub credential: Value,
}

/// Finish signing in with a passkey.
#[utoipa::path(
    post,
    path = "/auth/local/passkey/finish",
    operation_id = "finishPasskeySignIn",
    tag = "auth",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = PasskeySignInFinish,
    responses(
        (
            status = 200,
            description = "The user is signed in",
            body = SignedIn,
            headers(("Set-Cookie" = String, description = "The new `__Host-sc_session` cookie, and `__Host-sc_flow` cleared"))
        ),
        (status = 400, response = PasskeySignInFinishBadRequest),
        (status = 401, response = PasskeySignInFinishUnauthorized),
        (status = 403, response = PasskeySignInFinishForbidden),
        (status = 404, response = PasskeySignInFinishNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 429, response = crate::openapi::RateLimited),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn finish_passkey_sign_in(
    _limited: RateLimited<SignIn>,
    org: RequestOrg,
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<PasskeySignInFinish>,
) -> Result<Response, ApiError> {
    let scope = org.scope();
    require_method(&state.db, scope, Method::Passkey).await?;
    let now = state.clock.now();
    let mut tx = state.db.begin().await.map_err(|e| ApiError::internal(&e))?;
    let flow = flow::finish(
        &mut *tx,
        &state.keys,
        scope,
        &headers,
        Kind::PasskeySignIn,
        now,
    )
    .await;
    let flow = match flow {
        Ok(flow) => flow,
        Err(error) => return flow_refused(tx, error, &SIGN_IN_FLOW).await,
    };
    let pending: Pending =
        serde_json::from_slice(&flow.payload).map_err(|e| ApiError::internal(&e))?;
    let attempt = Attempt {
        state: &state,
        scope,
        user_id: flow.user_id,
        pending: &pending,
        credential: &body.credential,
        headers: &headers,
        now,
    };
    let response = match sign_in(&mut tx, attempt).await {
        Ok(started) => signed_in(started, flow.return_to.as_str()),
        Err(Outcome::Refused(refusal)) => with_flow_cleared(refusal.into_response()),
        Err(Outcome::Failed(error)) => return Err(error),
    };
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    Ok(response)
}

/// What [`sign_in`] needs besides the transaction.
struct Attempt<'a> {
    state: &'a AppState,
    scope: OrgScope,
    /// The account the flow was started for; `None` when the address had none.
    user_id: Option<Uuid>,
    pending: &'a Pending,
    credential: &'a Value,
    headers: &'a HeaderMap,
    now: SystemTime,
}

/// Steps 1 to 6 of the module documentation, in a savepoint of `tx` that only a sign-in
/// releases.
async fn sign_in(
    tx: &mut Transaction<'_, Postgres>,
    attempt: Attempt<'_>,
) -> Result<session::Started, Outcome> {
    let scope = attempt.scope;
    let mut sp = tx.begin().await.map_err(|e| Outcome::internal(&e))?;
    let Some(user_id) = attempt.user_id else {
        return Err(refused(
            "the sign-in was started for an address with no account",
        ));
    };
    let assertion = Assertion::parse(attempt.credential)
        .map_err(|_| refused("the credential is not a passkey assertion"))?;
    let stored = passkeys::lock_for_sign_in(&mut sp, scope, assertion.credential_id())
        .await
        .map_err(|e| Outcome::internal(&e))?
        .ok_or_else(|| refused("no passkey of the organization has the credential id"))?;
    if stored.user_id != user_id {
        return Err(refused("the passkey is another user's"));
    }
    let user = users::get(&mut *sp, scope, user_id)
        .await
        .map_err(|e| Outcome::internal(&e))?
        .ok_or_else(|| refused("the sign-in's account no longer exists"))?;
    let party = relying_party(attempt.state, &mut sp, scope)
        .await
        .map_err(Outcome::Failed)?;
    let verified = party
        .finish_authentication(
            &assertion,
            &attempt.pending.state,
            user_id,
            &stored.credential,
        )
        .map_err(|error| verification_refused(error, stored.id))?;
    if user.status == Status::Suspended {
        return Err(Outcome::Refused(suspended()));
    }
    passkeys::record_sign_in(&mut sp, scope, stored.id, &verified.credential, attempt.now)
        .await
        .map_err(|e| Outcome::internal(&e))?;
    let started = session::start_on(&mut sp, scope, user_id, attempt.headers, attempt.now)
        .await
        .map_err(|e| Outcome::internal(&e))?;
    sp.commit().await.map_err(|e| Outcome::internal(&e))?;
    tracing::info!(%user_id, passkey_id = %stored.id, "passkey sign-in");
    Ok(started)
}

/// 401 `auth.passkey_invalid`, logged with `reason` and nothing the client sent.
fn refused(reason: &'static str) -> Outcome {
    tracing::info!(reason, "passkey sign-in refused");
    Outcome::Refused(passkey_invalid())
}

/// A failed verification: the client's or an attacker's, and a server fault only when the
/// stored state or credential does not parse. A counter that did not go up is logged as a
/// warning, since two copies of the passkey may exist.
fn verification_refused(error: webauthn::SignInError, passkey_id: Uuid) -> Outcome {
    match error {
        webauthn::SignInError::CounterNotIncreased { .. } => {
            tracing::warn!(%passkey_id, %error, "passkey sign-in refused: the sign counter did not go up");
            Outcome::Refused(passkey_invalid())
        }
        webauthn::SignInError::Refused(_)
        | webauthn::SignInError::UserHandleMismatch
        | webauthn::SignInError::WrongCredential => {
            tracing::info!(%passkey_id, %error, "passkey sign-in refused");
            Outcome::Refused(passkey_invalid())
        }
        webauthn::SignInError::State(_)
        | webauthn::SignInError::Stored(_)
        | webauthn::SignInError::Serialize(_) => Outcome::internal(&error),
    }
}

fn passkey_invalid() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        code::AUTH_PASSKEY_INVALID,
        "The passkey did not sign you in. Start again and choose a passkey of this account.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_passkey_is_401_passkey_invalid() {
        let problem = passkey_invalid().problem();
        assert_eq!(
            (problem.status, problem.code.as_str()),
            (401, "auth.passkey_invalid")
        );
    }
}
