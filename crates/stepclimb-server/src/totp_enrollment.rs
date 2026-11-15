// SPDX-License-Identifier: AGPL-3.0-or-later
//! Setting up TOTP during a password sign-in, while the organization requires TOTP of password
//! accounts (docs/specs/m0/04-local-accounts.md B15, B19, B20, Q11 (a)).
//!
//! When the organization requires TOTP (`LocalSignIn::requires_totp`) and a right password
//! belongs to a user without confirmed TOTP, the password route (`crate::password`) calls
//! [`begin`] in its sign-in transaction instead of creating a session: it stores a new
//! unconfirmed secret for the user (replacing an unconfirmed one, as `POST /api/v1/me/totp`
//! does) and starts a `password_totp_enroll` flow naming the user, the password's generation and
//! the SHA-256 of the sealed secret it stored, which binds the flow to that secret.
//! The answer is 200 `{next: "totp_enroll", otpauth_uri, secret}` with the flow cookie and
//! `no-store`, and no session or device cookie.
//!
//! `POST /auth/local/totp/enrollment` ([`enroll_totp_at_sign_in`]) takes `RateLimited<SignIn>`
//! first, answers 404 `auth.method_disabled` unless TOTP is offered, then takes the flow, which
//! only a `password_totp_enroll` flow of the organization satisfies, so the code step's flow
//! cannot stand in for it nor it for the code step's. In a savepoint it locks the flow's user and
//! login and refuses (400 `auth.flow_invalid`) a flow whose password changed, whose user
//! confirmed TOTP in the meantime, or whose waiting secret is gone or was replaced (by another
//! password sign-in's set-up or by `POST /api/v1/me/totp`): the code the user typed belongs to a
//! secret this flow no longer names, so they start again rather than retry. It takes the failed-check
//! budgets of the password check, checks the code against the waiting secret (no recovery code:
//! there are none yet), confirms it with ten new recovery codes (`totp.enable`, the user as the
//! actor), and completes the sign-in through admission's local entry. The answer is 200
//! `{return_to, recovery_codes}` with the session, a device cookie, the flow cookie cleared and
//! `no-store`: the codes are shown this once.
//!
//! The flow ends with the session it gives, at its 10-minute expiry, or at a refusal other than
//! a wrong code. A wrong code (401 `auth.totp_invalid`) counts against the budgets and rolls
//! everything back, leaving the flow usable: the person already proved the password, and the
//! code only shows their app holds the secret they were just shown, so a typo must not cost them
//! a new secret to scan. A spent budget (429) and a server fault roll back too. An admission
//! refusal (403) commits the flow's deletion and nothing else, so TOTP stays unset.
//!
//! Nothing is logged but the user id; the secret, the URI and the codes never reach a log line.

use axum::{
    Json,
    extract::{FromRequest, Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Acquire, PgConnection, Postgres, Transaction};
use stepclimb_auth::totp::{self as algorithm, Secret};
use stepclimb_core::{
    OrgScope,
    audit::Actor,
    auth_flows::Kind,
    id::Uuid,
    identities, local_logins,
    local_sign_in::Method,
    local_totp::{self, Totp},
    organizations,
    users::User,
};
use utoipa::ToSchema;

use crate::{
    AppState,
    admission::{self, Completion},
    audit::ClientAddress,
    error::ApiError,
    extract::ApiJson,
    flow::{self, FlowStart},
    local_sign_in::require_method,
    openapi::problem_responses,
    passkey_enrollment::{flow_refused, with_flow_cleared},
    password::with_session_and_device,
    password_check::{Attempt, Refusal, Subject, internal, take_budgets},
    rate_limit::{AccountCheck, RateLimited, SignIn, TooManyRequests},
    request_org::RequestOrg,
    session,
    totp::{
        NextStep, SECOND_STEP_FLOW, TotpCode, accept_code, code::AUTH_TOTP_INVALID, flow_changed,
        no_store_with_flow, seal_secret,
    },
};

/// Confirming TOTP set up during sign-in.
pub const TOTP_ENROLLMENT_PATH: &str = "/auth/local/totp/enrollment";

problem_responses! {
    TotpEnrollmentBadRequest => "`request.malformed`: the body is not `{code}`. `auth.flow_invalid`: no password sign-in is waiting for TOTP set-up in this browser, it was already finished, or the password or TOTP changed since; sign in with the password again. `auth.flow_expired`: the password was checked more than 10 minutes ago; sign in with the password again.",
    TotpEnrollmentUnauthorized => "`auth.totp_invalid`: the code is wrong, or not the current one, for the secret the password step answered. The set-up is still waiting: try the current code again.",
}

/// A right password whose account must set up TOTP first: no session exists yet. Shown once.
// No `Debug`, so the secret cannot reach a log line through `{:?}`.
#[derive(Serialize, ToSchema)]
pub struct EnrollmentRequired {
    /// `totp_enroll`: send a code from the app to `POST /auth/local/totp/enrollment` in this
    /// browser within 10 minutes.
    pub next: NextStep,
    /// The URI to show as a QR code for the authenticator app.
    pub otpauth_uri: String,
    /// The secret in base32, for typing into the app instead.
    pub secret: String,
}

/// What the set-up flow keeps, sealed: the password's generation when it was checked, and the
/// SHA-256 of the sealed secret the set-up stored, so a replaced secret ends the set-up.
#[derive(Serialize, Deserialize)]
pub struct SetUpPending {
    pub generation: i64,
    pub secret_hash: [u8; 32],
}

/// The SHA-256 of a sealed secret, as [`SetUpPending::secret_hash`] keeps it.
#[must_use]
pub fn sealed_secret_hash(sealed: &[u8]) -> [u8; 32] {
    Sha256::digest(sealed).into()
}

/// TOTP is on and the user is signed in; the recovery codes are shown once.
// No `Debug`, so the codes cannot reach a log line through `{:?}`.
#[derive(Serialize, ToSchema)]
pub struct EnrolledSignedIn {
    /// The path on this site to open next.
    pub return_to: String,
    /// Ten single-use codes that stand in for a TOTP code when the app is lost.
    pub recovery_codes: Vec<String>,
}

/// In the password route's transaction, after the user's lock: give the user `user` a new
/// unconfirmed secret and start the set-up flow for their password of generation `generation`
/// (see the module documentation). Returns the answer to send.
pub(crate) async fn begin(
    conn: &mut PgConnection,
    state: &AppState,
    scope: OrgScope,
    user: &User,
    generation: i64,
    return_to: Option<&str>,
) -> Result<Response, ApiError> {
    let now = state.clock.now();
    let secret = Secret::generate().map_err(internal)?;
    let sealed = seal_secret(&state.keys, scope, user.id, &secret).map_err(internal)?;
    local_totp::start(&mut *conn, scope, user.id, &sealed, now)
        .await
        .map_err(internal)?;
    let pending = SetUpPending {
        generation,
        secret_hash: sealed_secret_hash(&sealed),
    };
    let payload = serde_json::to_vec(&pending).map_err(internal)?;
    let start = FlowStart {
        kind: Kind::PasswordTotpEnroll,
        user_id: Some(user.id),
        provider_id: None,
        payload: &payload,
        return_to,
    };
    let flow = flow::start(&mut *conn, &state.keys, scope, start, now)
        .await
        .map_err(internal)?;
    let organization = organizations::get(&mut *conn, scope)
        .await
        .map_err(internal)?;
    let account = user.email.as_deref().unwrap_or(&user.display_name);
    let body = EnrollmentRequired {
        next: NextStep::TotpEnroll,
        otpauth_uri: secret.otpauth_uri(&organization.name, account),
        secret: secret.expose_base32(),
    };
    Ok(no_store_with_flow(Json(body).into_response(), flow))
}

/// Confirm TOTP set up during sign-in and finish signing in.
#[utoipa::path(
    post,
    path = "/auth/local/totp/enrollment",
    operation_id = "enrollTotpAtSignIn",
    tag = "auth",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = TotpCode,
    responses(
        (
            status = 200,
            description = "TOTP is on and the user is signed in; the recovery codes are shown once",
            body = EnrolledSignedIn,
            headers(
                ("Cache-Control" = String, description = "`no-store`"),
                ("Set-Cookie" = String, description = "The new `__Host-sc_session` cookie, the `__Host-sc_device` cookie, and `__Host-sc_flow` cleared")
            )
        ),
        (status = 400, response = TotpEnrollmentBadRequest),
        (status = 401, response = TotpEnrollmentUnauthorized),
        (status = 403, response = crate::totp::TotpSignInForbidden),
        (status = 404, response = crate::totp::TotpNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 429, response = crate::openapi::RateLimited),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn enroll_totp_at_sign_in(
    limited: RateLimited<SignIn>,
    org: RequestOrg,
    client: ClientAddress,
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
) -> Result<Response, Refusal> {
    let scope = org.scope();
    require_method(&state.db, scope, Method::Totp).await?;
    let ApiJson(body) = ApiJson::<TotpCode>::from_request(request, &state).await?;
    let now = state.clock.now();
    let mut tx = state.db.begin().await.map_err(internal)?;
    let kind = Kind::PasswordTotpEnroll;
    let flow = match flow::finish(&mut *tx, &state.keys, scope, &headers, kind, now).await {
        Ok(flow) => flow,
        Err(error) => return Ok(flow_refused(tx, error, &SECOND_STEP_FLOW).await?),
    };
    let pending: SetUpPending = serde_json::from_slice(&flow.payload).map_err(internal)?;
    let attempt = Attempt {
        state: &state,
        scope,
        headers: &headers,
        now,
    };
    let step = Enrollment {
        user_id: flow.user_id,
        pending: &pending,
        code: &body.code,
        ip: client.0,
    };
    let response = match confirm(&mut tx, &limited, &attempt, step).await {
        Ok(confirmed) => {
            tracing::info!(user_id = %confirmed.user_id, "TOTP set up at sign-in");
            let body = EnrolledSignedIn {
                return_to: flow.return_to.as_str().to_owned(),
                recovery_codes: confirmed.recovery_codes,
            };
            let user_id = confirmed.user_id;
            let response = Json(body).into_response();
            let response = with_session_and_device(
                &state,
                scope,
                user_id,
                pending.generation,
                confirmed.started,
                response,
                now,
            )?;
            with_flow_cleared(response)
        }
        Err(Step::Refused(refusal)) => with_flow_cleared(refusal.into_response()),
        // A wrong code, a spent budget and a fault roll everything back: the flow stays usable.
        Err(Step::WrongCode) => return Err(setup_code_invalid().into()),
        Err(Step::Limited(limited)) => return Err(limited.into()),
        Err(Step::Failed(error)) => return Err(error.into()),
    };
    tx.commit().await.map_err(internal)?;
    Ok(response)
}

/// The set-up's inputs besides the transaction.
struct Enrollment<'a> {
    user_id: Option<Uuid>,
    pending: &'a SetUpPending,
    code: &'a str,
    ip: std::net::IpAddr,
}

/// A confirmed set-up: the user, their session and the recovery codes to show.
struct Confirmed {
    user_id: Uuid,
    started: session::Started,
    recovery_codes: Vec<String>,
}

/// How the set-up did not sign in.
enum Step {
    /// The client's refusal: answered after the flow's deletion commits.
    Refused(ApiError),
    /// The code was not accepted: everything rolls back, the failure stays counted.
    WrongCode,
    /// A failed-check budget is spent: everything rolls back.
    Limited(TooManyRequests),
    /// A server fault: everything rolls back.
    Failed(ApiError),
}

/// In a savepoint of `tx` that only a sign-in releases: lock the flow's user and login, refuse a
/// flow whose password or TOTP changed since, take the budgets, check the code against the
/// waiting secret, confirm it with new recovery codes and complete the sign-in.
async fn confirm(
    tx: &mut Transaction<'_, Postgres>,
    limited: &RateLimited<SignIn>,
    attempt: &Attempt<'_>,
    step: Enrollment<'_>,
) -> Result<Confirmed, Step> {
    let (scope, now) = (attempt.scope, attempt.now);
    let failed = |e: &dyn std::error::Error| Step::Failed(ApiError::internal(e));
    let mut sp = tx.begin().await.map_err(|e| failed(&e))?;
    let user_id = step.user_id.ok_or_else(|| Step::Refused(flow_changed()))?;
    let totp = lock_waiting(&mut sp, scope, user_id, step.pending).await?;
    let login = local_logins::get(&mut *sp, scope, user_id)
        .await
        .map_err(|e| failed(&e))?;
    let subject = Subject {
        email: login.as_ref().and_then(|login| login.email.as_ref()),
        login: login.as_ref(),
        signed_in: None,
    };
    let taken = take_budgets(limited, attempt, &subject).map_err(Step::Limited)?;
    let accepted = accept_code(&mut sp, attempt, user_id, &totp, step.code, false)
        .await
        .map_err(Step::Failed)?;
    if accepted.is_none() {
        // The budgets taken keep this failure counted.
        drop(taken);
        tracing::info!(%user_id, "TOTP set-up at sign-in refused: the code was not accepted");
        return Err(Step::WrongCode);
    }
    taken.into_iter().for_each(AccountCheck::passed);
    let codes = algorithm::generate_recovery_codes().map_err(|e| failed(&e))?;
    let hashes: Vec<[u8; 32]> = codes.iter().map(|code| *code.hash().as_bytes()).collect();
    let actor = Actor::User {
        id: user_id,
        ip: step.ip,
    };
    local_totp::confirm(&mut sp, scope, user_id, &hashes, now, actor)
        .await
        .map_err(|e| failed(&e))?;
    let started =
        match admission::complete_local(&mut sp, scope, user_id, now, attempt.headers).await {
            Ok(started) => started,
            Err(Completion::Refused(refusal)) => return Err(Step::Refused(refusal.into_error())),
            Err(Completion::Failed(error)) => return Err(Step::Failed(error)),
        };
    sp.commit().await.map_err(|e| failed(&e))?;
    let recovery_codes = codes
        .iter()
        .map(|code| code.expose_secret().to_owned())
        .collect();
    Ok(Confirmed {
        user_id,
        started,
        recovery_codes,
    })
}

/// Lock the user `user_id` and their login, and return their TOTP if it still waits for this
/// flow: a password of `pending`'s generation and, unconfirmed, the secret whose sealed form
/// hashes to `pending`'s. Anything else (a changed password, TOTP confirmed, the secret gone or
/// replaced) is the flow's refusal.
async fn lock_waiting(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
    pending: &SetUpPending,
) -> Result<Totp, Step> {
    match identities::lock_user(&mut *conn, scope, user_id).await {
        Ok(_) => {}
        Err(identities::Error::UserNotFound) => return Err(Step::Refused(flow_changed())),
        Err(other) => return Err(Step::Failed(internal(other))),
    }
    local_totp::lock(conn, scope, user_id)
        .await
        .map_err(|e| Step::Failed(internal(e)))?
        .filter(|totp| {
            let same_secret = totp
                .sealed_secret
                .as_deref()
                .is_some_and(|sealed| sealed_secret_hash(sealed) == pending.secret_hash);
            !totp.confirmed && same_secret && totp.password_generation == pending.generation
        })
        .ok_or_else(|| Step::Refused(flow_changed()))
}

/// 401 `auth.totp_invalid`, saying the set-up still waits.
fn setup_code_invalid() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        AUTH_TOTP_INVALID,
        "The code is wrong or not the current one. Check the app's clock and enter its current code.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_names_the_set_up_step() {
        let body = serde_json::to_value(EnrollmentRequired {
            next: NextStep::TotpEnroll,
            otpauth_uri: "otpauth://totp/x".into(),
            secret: "ABC".into(),
        })
        .unwrap();
        assert_eq!(body["next"], "totp_enroll");
    }

    #[test]
    fn a_wrong_code_is_401() {
        let problem = setup_code_invalid().problem();
        assert_eq!(
            (problem.status, problem.code.as_str()),
            (StatusCode::UNAUTHORIZED.as_u16(), "auth.totp_invalid")
        );
    }
}
