// SPDX-License-Identifier: AGPL-3.0-or-later
//! TOTP, the second step after a local account's password: enrollment, confirmation with
//! recovery codes, the sign-in step and disabling (docs/specs/m0/04-local-accounts.md B2, B15,
//! B19 to B21, B23, B24, Q11).
//!
//! Every route answers 404 `auth.method_disabled` unless the organization offers TOTP, which it
//! does only while it offers the password too (`LocalSignIn::offers`). Codes are Q11's: SHA-1,
//! 6 digits, 30 seconds, one step of drift either way, checked by `stepclimb_auth::totp` at the
//! controllable clock's time. A code is accepted only for a step after the user's last accepted
//! one (`local_logins.totp_last_step`), so it is used once (RFC 6238 section 5.2). The secret is
//! sealed with the key ring ([`seal_secret`]) bound to the organization, `local_logins`, the
//! user and `totp_secret`, so a sealed value copied to another row does not open.
//!
//! `POST /api/v1/me/totp` ([`start_my_totp`], B19) gives a signed-in user with a password a new
//! unconfirmed secret, replacing an unconfirmed one, and answers it once with its `otpauth://`
//! URI. `POST /api/v1/me/totp/confirm` ([`confirm_my_totp`]) confirms it with a code and answers
//! the ten recovery codes once (Q11); the server keeps their hashes. `POST
//! /api/v1/me/totp/disable` ([`disable_my_totp`], Q11) removes TOTP given a current code or an
//! unused recovery code. The three take a browser session only (`SessionOnly`), as every route
//! that manages a user's own sign-in does.
//!
//! The password route (`crate::password`) asks for the second step when the password is right
//! and the user has confirmed TOTP: [`begin_second_step`] starts a `password_totp` flow naming
//! the user and the password's generation, and the answer is 200 `{next: "totp"}` with no
//! session (B15). `POST /auth/local/totp` ([`sign_in_with_totp`], B20) takes that flow, then in a
//! savepoint locks the user and their login, refuses a flow whose password changed or whose
//! TOTP was disabled since (400 `auth.flow_invalid`), takes the failed-check budgets of the
//! password check (`password_check::take_budgets`, B18), accepts a current code or an unused
//! recovery code, and completes the sign-in through admission's local entry. The flow is used
//! once whatever the outcome: a wrong code (401 `auth.totp_invalid`, counted against the
//! account) commits the flow's deletion, so every new guess needs the password again. Only a
//! spent budget (429) and a server fault roll the flow's deletion back.
//!
//! The code-checking routes take `RateLimited<SignIn>` first. No step logs the secret, the URI,
//! a code or a recovery code; `totp.enable` and `totp.disable` audit entries name the user.

use std::time::SystemTime;

use axum::{
    Json,
    extract::{FromRequest, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::{Acquire, PgConnection, PgExecutor, Postgres, Transaction};
use stepclimb_auth::{
    encryption::{EncryptError, KeyRing},
    totp::{self, Code, Secret},
};
use stepclimb_core::{
    OrgScope,
    auth_flows::Kind,
    id::Uuid,
    identities, local_logins,
    local_sign_in::{self as switches, Method},
    local_totp::{self, Totp},
    organizations,
};
use utoipa::ToSchema;

use crate::{
    AppState, admission,
    audit::ClientAddress,
    error::ApiError,
    extract::ApiJson,
    flow::{self, FlowStart},
    local_sign_in::require_method,
    openapi::problem_responses,
    passkey_enrollment::{FlowDetails, Outcome, flow_refused, with_flow_cleared},
    password::signed_in_with_device,
    password_check::{Attempt, Refusal, Subject, internal, take_budgets},
    rate_limit::{AccountCheck, RateLimited, SignIn, TooManyRequests},
    request_org::RequestOrg,
    require::SessionOnly,
    session,
};

/// The second sign-in step.
pub const TOTP_SIGN_IN_PATH: &str = "/auth/local/totp";

/// Starting TOTP enrollment.
pub const MY_TOTP_PATH: &str = "/api/v1/me/totp";

/// Confirming it.
pub const MY_TOTP_CONFIRM_PATH: &str = "/api/v1/me/totp/confirm";

/// Disabling it.
pub const MY_TOTP_DISABLE_PATH: &str = "/api/v1/me/totp/disable";

/// The codes this module produces.
pub mod code {
    /// The code is wrong, was used already, is outside the window, or is no unused recovery
    /// code (401).
    pub const AUTH_TOTP_INVALID: &str = "auth.totp_invalid";
    /// TOTP needs a password, and the user has none (409).
    pub const LOCAL_PASSWORD_REQUIRED: &str = "local.password_required";
    /// No unconfirmed TOTP waits for a code (409).
    pub const LOCAL_TOTP_NOT_STARTED: &str = "local.totp_not_started";
    /// TOTP is already on; disable it first (409).
    pub const LOCAL_TOTP_ENABLED: &str = "local.totp_enabled";
    /// TOTP is not on, so there is nothing to disable (409).
    pub const LOCAL_TOTP_NOT_ENABLED: &str = "local.totp_not_enabled";
}

/// The table and column the secret is sealed for (`stepclimb_auth::encryption` context rules).
const SECRET_TABLE: &[u8] = b"local_logins";
const SECRET_COLUMN: &[u8] = b"totp_secret";

problem_responses! {
    TotpNotFound => "`auth.method_disabled`: TOTP is switched off for this organization, or password sign-in is, which TOTP follows. `org.not_found`: the request's organization cannot be determined.",
    TotpSignInBadRequest => "`request.malformed`: the body is not `{code}`. `auth.flow_invalid`: no password sign-in is waiting for a code in this browser, it was already finished, or the password or TOTP changed since; sign in with the password again. `auth.flow_expired`: the password was checked more than 10 minutes ago; sign in with the password again.",
    TotpSignInUnauthorized => "`auth.totp_invalid`: the code is wrong, already used, or not the current one, and it is no unused recovery code. Sign in with the password again.",
    TotpSignInForbidden => "`auth.suspended`: the account is suspended. `auth.csrf_refused`: the request did not come from this site.",
    MyTotpForbidden => "`auth.session_required`: the request carries a bearer credential; only a signed-in browser session manages TOTP. `auth.csrf_refused`: the request did not come from this site.",
    MyTotpStartConflict => "`local.password_required`: the account has no password, which TOTP follows. `local.totp_enabled`: TOTP is already on; disable it first.",
    MyTotpUnauthorized => "`auth.unauthenticated`, `auth.session_expired`, `auth.suspended`: no valid session. `auth.totp_invalid`: the code is wrong, already used, or not the current one.",
    MyTotpConfirmConflict => "`local.totp_not_started`: no TOTP enrollment is waiting for a code; start one. `local.totp_enabled`: TOTP is already on.",
    MyTotpDisableConflict => "`local.totp_not_enabled`: TOTP is not on.",
}

const SECOND_STEP_FLOW: FlowDetails = FlowDetails {
    expired: "The password was checked more than 10 minutes ago. Sign in with your password again.",
    invalid: "No sign-in is waiting for a code in this browser. Sign in with your password again.",
};

/// What the second step's flow keeps, sealed: the password's generation when it was checked,
/// so a password changed in between ends the sign-in.
#[derive(Serialize, Deserialize)]
struct Pending {
    generation: i64,
}

/// The next step a password sign-in needs.
#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum NextStep {
    /// A TOTP code, sent to `POST /auth/local/totp`.
    Totp,
}

/// A right password whose account also needs a TOTP code: no session exists yet.
#[derive(Debug, Serialize, ToSchema)]
pub struct CodeRequired {
    /// Send the code to `POST /auth/local/totp` in this browser within 10 minutes.
    pub next: NextStep,
}

/// A TOTP code, or a recovery code where one is accepted. Never logged.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TotpCode {
    /// The 6 digits the authenticator app shows (spaces ignored), or, to sign in or disable
    /// TOTP, one of the recovery codes (`ABCD-EFGH-IJKL-MNOP`, any letter case).
    pub code: String,
}

/// A new TOTP secret, shown once.
// No `Debug`, so the secret cannot reach a log line through `{:?}`.
#[derive(Serialize, ToSchema)]
pub struct TotpEnrollment {
    /// The URI to show as a QR code for the authenticator app.
    pub otpauth_uri: String,
    /// The secret in base32, for typing into the app instead.
    pub secret: String,
}

/// The recovery codes of a confirmed TOTP, shown once.
// No `Debug`, so the codes cannot reach a log line through `{:?}`.
#[derive(Serialize, ToSchema)]
pub struct TotpConfirmed {
    /// Ten single-use codes that stand in for a TOTP code when the app is lost.
    pub recovery_codes: Vec<String>,
}

/// `secret`, sealed for the `totp_secret` column of the user `user_id` of the scope's
/// organization.
pub fn seal_secret(
    keys: &KeyRing,
    scope: OrgScope,
    user_id: Uuid,
    secret: &Secret,
) -> Result<Vec<u8>, EncryptError> {
    let org_id = scope.org_id();
    keys.encrypt(secret.expose_bytes(), &secret_context(&org_id, &user_id))
}

fn open_secret(
    keys: &KeyRing,
    scope: OrgScope,
    user_id: Uuid,
    sealed: &[u8],
) -> Result<Secret, ApiError> {
    let org_id = scope.org_id();
    let bytes = keys
        .decrypt(sealed, &secret_context(&org_id, &user_id))
        .map_err(internal)?;
    Secret::from_decrypted(bytes).map_err(internal)
}

/// `[org_id, "local_logins", user_id, "totp_secret"]`.
fn secret_context<'a>(org_id: &'a Uuid, user_id: &'a Uuid) -> [&'a [u8]; 4] {
    [
        org_id.as_bytes(),
        SECRET_TABLE,
        user_id.as_bytes(),
        SECRET_COLUMN,
    ]
}

/// Whether the password sign-in of `totp`'s user needs a code (B15): the organization offers
/// TOTP and the user confirmed it. An unconfirmed secret asks for nothing, and while TOTP is
/// switched off a confirmed one asks for nothing either (B2: it works again once switched on).
pub(crate) async fn needs_code(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    totp: Option<&Totp>,
) -> Result<bool, ApiError> {
    if !totp.is_some_and(|totp| totp.confirmed) {
        return Ok(false);
    }
    let local = switches::get(db, scope).await.map_err(internal)?;
    Ok(local.offers(Method::Totp))
}

/// Start the second step for the user `user_id`, whose password of generation `generation` was
/// just verified, at `now`: a `password_totp` flow keeping `return_to` for the sign-in it ends.
pub async fn begin_second_step(
    db: impl PgExecutor<'_>,
    keys: &KeyRing,
    scope: OrgScope,
    user_id: Uuid,
    generation: i64,
    return_to: Option<&str>,
    now: SystemTime,
) -> Result<flow::Started, ApiError> {
    let payload = serde_json::to_vec(&Pending { generation }).map_err(internal)?;
    let start = FlowStart {
        kind: Kind::PasswordTotp,
        user_id: Some(user_id),
        provider_id: None,
        payload: &payload,
        return_to,
    };
    flow::start(db, keys, scope, start, now)
        .await
        .map_err(internal)
}

/// 200 `{next: "totp"}` with the second step's flow cookie and `no-store`.
pub(crate) fn code_required(flow: flow::Started) -> Response {
    let mut response = Json(CodeRequired {
        next: NextStep::Totp,
    })
    .into_response();
    let headers = response.headers_mut();
    headers.append(header::SET_COOKIE, flow.set_cookie);
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Finish signing in with a TOTP code after the password.
#[utoipa::path(
    post,
    path = "/auth/local/totp",
    operation_id = "signInWithTotp",
    tag = "auth",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = TotpCode,
    responses(
        (
            status = 200,
            description = "The user is signed in",
            body = crate::passkey_enrollment::SignedIn,
            headers(
                ("Cache-Control" = String, description = "`no-store`"),
                ("Set-Cookie" = String, description = "The new `__Host-sc_session` cookie, the `__Host-sc_device` cookie, and `__Host-sc_flow` cleared")
            )
        ),
        (status = 400, response = TotpSignInBadRequest),
        (status = 401, response = TotpSignInUnauthorized),
        (status = 403, response = TotpSignInForbidden),
        (status = 404, response = TotpNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 429, response = crate::openapi::RateLimited),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn sign_in_with_totp(
    limited: RateLimited<SignIn>,
    org: RequestOrg,
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
) -> Result<Response, Refusal> {
    let scope = org.scope();
    require_method(&state.db, scope, Method::Totp).await?;
    let ApiJson(body) = ApiJson::<TotpCode>::from_request(request, &state).await?;
    let now = state.clock.now();
    let mut tx = state.db.begin().await.map_err(internal)?;
    let kind = Kind::PasswordTotp;
    let flow = match flow::finish(&mut *tx, &state.keys, scope, &headers, kind, now).await {
        Ok(flow) => flow,
        Err(error) => return Ok(flow_refused(tx, error, &SECOND_STEP_FLOW).await?),
    };
    let pending: Pending = serde_json::from_slice(&flow.payload).map_err(internal)?;
    let attempt = Attempt {
        state: &state,
        scope,
        headers: &headers,
        now,
    };
    let step = SecondStep {
        user_id: flow.user_id,
        pending: &pending,
        code: &body.code,
    };
    let response = match second_step(&mut tx, &limited, &attempt, step).await {
        Ok((user_id, started)) => {
            tracing::info!(%user_id, "TOTP sign-in");
            let to = flow.return_to.as_str();
            let response = signed_in_with_device(
                &state,
                scope,
                user_id,
                pending.generation,
                started,
                to,
                now,
            )?;
            with_flow_cleared(response)
        }
        Err(Step::Refused(refusal)) => with_flow_cleared(refusal.into_response()),
        // Nothing was checked: the flow stays usable once the budget allows.
        Err(Step::Limited(limited)) => return Err(limited.into()),
        Err(Step::Failed(error)) => return Err(error.into()),
    };
    tx.commit().await.map_err(internal)?;
    Ok(response)
}

/// The second step's inputs besides the transaction.
struct SecondStep<'a> {
    user_id: Option<Uuid>,
    pending: &'a Pending,
    code: &'a str,
}

/// How the second step did not sign in.
enum Step {
    /// The client's refusal: answered after the flow's deletion commits.
    Refused(ApiError),
    /// A failed-check budget is spent: answered with everything rolled back.
    Limited(TooManyRequests),
    /// A server fault: everything rolls back.
    Failed(ApiError),
}

impl From<Outcome> for Step {
    fn from(outcome: Outcome) -> Self {
        match outcome {
            Outcome::Refused(error) => Self::Refused(error),
            Outcome::Failed(error) => Self::Failed(error),
        }
    }
}

/// In a savepoint of `tx` that only a sign-in releases: lock the flow's user and login, refuse
/// a flow whose password or TOTP changed since, take the budgets, check the code and complete
/// the sign-in. Returns the user and their session.
async fn second_step(
    tx: &mut Transaction<'_, Postgres>,
    limited: &RateLimited<SignIn>,
    attempt: &Attempt<'_>,
    step: SecondStep<'_>,
) -> Result<(Uuid, session::Started), Step> {
    let scope = attempt.scope;
    let mut sp = tx.begin().await.map_err(|e| Step::Failed(internal(e)))?;
    let user_id = step.user_id.ok_or_else(|| Step::Refused(flow_changed()))?;
    match identities::lock_user(&mut sp, scope, user_id).await {
        Ok(_) => {}
        Err(identities::Error::UserNotFound) => return Err(Step::Refused(flow_changed())),
        Err(other) => return Err(Step::Failed(internal(other))),
    }
    let totp = local_totp::lock(&mut sp, scope, user_id)
        .await
        .map_err(|e| Step::Failed(internal(e)))?
        .filter(|totp| totp.confirmed && totp.password_generation == step.pending.generation)
        .ok_or_else(|| Step::Refused(flow_changed()))?;
    let login = local_logins::get(&mut *sp, scope, user_id)
        .await
        .map_err(|e| Step::Failed(internal(e)))?;
    let subject = Subject {
        email: login.as_ref().and_then(|login| login.email.as_ref()),
        login: login.as_ref(),
        signed_in: None,
    };
    let taken = take_budgets(limited, attempt, &subject).map_err(Step::Limited)?;
    let accepted = accept_code(&mut sp, attempt, user_id, &totp, step.code, true)
        .await
        .map_err(Step::Failed)?;
    let Some(used) = accepted else {
        // The budgets taken keep this failure counted.
        drop(taken);
        tracing::info!(%user_id, "TOTP sign-in refused: the code was not accepted");
        return Err(Step::Refused(totp_invalid()));
    };
    taken.into_iter().for_each(AccountCheck::passed);
    if used == Used::RecoveryCode {
        tracing::info!(%user_id, "TOTP sign-in with a recovery code");
    }
    let started = admission::complete_local(&mut sp, scope, user_id, attempt.now, attempt.headers)
        .await
        .map_err(|completion| Step::from(Outcome::from(completion)))?;
    sp.commit().await.map_err(|e| Step::Failed(internal(e)))?;
    Ok((user_id, started))
}

/// What a code was accepted as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Used {
    Totp,
    RecoveryCode,
}

/// Accept `presented` for `totp`'s user: a current code for a step after the last accepted one,
/// whose step is then recorded, or, when `recovery` allows, an unused recovery code, which is
/// then marked used. `None` when it is neither.
async fn accept_code(
    conn: &mut PgConnection,
    attempt: &Attempt<'_>,
    user_id: Uuid,
    totp: &Totp,
    presented: &str,
    recovery: bool,
) -> Result<Option<Used>, ApiError> {
    let (scope, now) = (attempt.scope, attempt.now);
    if let Some(code) = Code::parse(presented) {
        let Some(sealed) = totp.sealed_secret.as_deref() else {
            return Ok(None);
        };
        let secret = open_secret(&attempt.state.keys, scope, user_id, sealed)?;
        let Some(step) = totp::verify(&secret, &code, now, totp.last_step) else {
            return Ok(None);
        };
        // The update records the step only if it is still later than the last one, so a code
        // that a concurrent request used first is refused here even without the locks.
        let recorded = local_totp::record_step(conn, scope, user_id, step, now)
            .await
            .map_err(internal)?;
        return Ok(recorded.then_some(Used::Totp));
    }
    let Some(hash) = totp::recovery_code_hash(presented).filter(|_| recovery) else {
        return Ok(None);
    };
    let used = local_totp::use_recovery_code(conn, scope, user_id, hash.as_bytes(), now)
        .await
        .map_err(internal)?;
    Ok(used.then_some(Used::RecoveryCode))
}

/// Start TOTP enrollment.
#[utoipa::path(
    post,
    path = "/api/v1/me/totp",
    operation_id = "startMyTotp",
    tag = "me",
    extensions(("x-stepclimb-mutates" = json!(true))),
    responses(
        (
            status = 200,
            description = "A new unconfirmed secret, shown once; it replaces an unconfirmed one",
            body = TotpEnrollment,
            headers(("Cache-Control" = String, description = "`no-store`"))
        ),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = MyTotpForbidden),
        (status = 404, response = TotpNotFound),
        (status = 409, response = MyTotpStartConflict),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn start_my_totp(
    session: SessionOnly,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    let principal = session.principal();
    let (scope, user_id) = (principal.scope(), principal.user_id);
    require_method(&state.db, scope, Method::Totp).await?;
    let secret = Secret::generate().map_err(internal)?;
    let sealed = seal_secret(&state.keys, scope, user_id, &secret).map_err(internal)?;
    let mut tx = state.db.begin().await.map_err(internal)?;
    let user = identities::lock_user(&mut tx, scope, user_id)
        .await
        .map_err(internal)?;
    local_totp::start(&mut tx, scope, user_id, &sealed, state.clock.now())
        .await
        .map_err(totp_error)?;
    let organization = organizations::get(&mut *tx, scope)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    tracing::info!(%user_id, "TOTP enrollment started");
    let account = user.email.as_deref().unwrap_or(&user.display_name);
    let body = TotpEnrollment {
        otpauth_uri: secret.otpauth_uri(&organization.name, account),
        secret: secret.expose_base32(),
    };
    Ok(no_store(Json(body).into_response()))
}

/// Confirm TOTP with a code from the app.
#[utoipa::path(
    post,
    path = "/api/v1/me/totp/confirm",
    operation_id = "confirmMyTotp",
    tag = "me",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = TotpCode,
    responses(
        (
            status = 200,
            description = "TOTP is on; the recovery codes are shown once",
            body = TotpConfirmed,
            headers(("Cache-Control" = String, description = "`no-store`"))
        ),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = MyTotpUnauthorized),
        (status = 403, response = MyTotpForbidden),
        (status = 404, response = TotpNotFound),
        (status = 409, response = MyTotpConfirmConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 429, response = crate::openapi::RateLimited),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn confirm_my_totp(
    limited: RateLimited<SignIn>,
    session: SessionOnly,
    client: ClientAddress,
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<TotpCode>,
) -> Result<Response, Refusal> {
    let principal = session.principal();
    let (scope, user_id) = (principal.scope(), principal.user_id);
    require_method(&state.db, scope, Method::Totp).await?;
    let attempt = attempt(&state, scope, &headers);
    let mut tx = state.db.begin().await.map_err(internal)?;
    let totp = lock_own(&mut tx, scope, user_id).await?;
    if totp.confirmed {
        return Err(totp_error(local_totp::Error::AlreadyConfirmed).into());
    }
    if totp.sealed_secret.is_none() {
        return Err(totp_error(local_totp::Error::NotStarted).into());
    }
    let taken = own_budgets(&mut tx, &limited, &attempt, user_id).await?;
    let accepted = accept_code(&mut tx, &attempt, user_id, &totp, &body.code, false).await?;
    if accepted.is_none() {
        drop(taken);
        tracing::info!(%user_id, "TOTP confirmation refused: the code was not accepted");
        return Err(totp_invalid().into());
    }
    taken.into_iter().for_each(AccountCheck::passed);
    let codes = totp::generate_recovery_codes().map_err(internal)?;
    let hashes: Vec<[u8; 32]> = codes.iter().map(|code| *code.hash().as_bytes()).collect();
    let actor = client.actor(principal);
    local_totp::confirm(&mut tx, scope, user_id, &hashes, attempt.now, actor)
        .await
        .map_err(totp_error)?;
    tx.commit().await.map_err(internal)?;
    tracing::info!(%user_id, "TOTP confirmed");
    let body = TotpConfirmed {
        recovery_codes: codes
            .iter()
            .map(|code| code.expose_secret().to_owned())
            .collect(),
    };
    Ok(no_store(Json(body).into_response()))
}

/// Turn TOTP off with a current code or a recovery code.
#[utoipa::path(
    post,
    path = "/api/v1/me/totp/disable",
    operation_id = "disableMyTotp",
    tag = "me",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = TotpCode,
    responses(
        (status = 204, description = "TOTP is off; its secret and recovery codes are deleted"),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = MyTotpUnauthorized),
        (status = 403, response = MyTotpForbidden),
        (status = 404, response = TotpNotFound),
        (status = 409, response = MyTotpDisableConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 429, response = crate::openapi::RateLimited),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn disable_my_totp(
    limited: RateLimited<SignIn>,
    session: SessionOnly,
    client: ClientAddress,
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<TotpCode>,
) -> Result<Response, Refusal> {
    let principal = session.principal();
    let (scope, user_id) = (principal.scope(), principal.user_id);
    require_method(&state.db, scope, Method::Totp).await?;
    let attempt = attempt(&state, scope, &headers);
    let mut tx = state.db.begin().await.map_err(internal)?;
    let totp = lock_own(&mut tx, scope, user_id).await?;
    if !totp.confirmed {
        return Err(totp_error(local_totp::Error::NotConfirmed).into());
    }
    let taken = own_budgets(&mut tx, &limited, &attempt, user_id).await?;
    let accepted = accept_code(&mut tx, &attempt, user_id, &totp, &body.code, true).await?;
    if accepted.is_none() {
        drop(taken);
        tracing::info!(%user_id, "TOTP disabling refused: the code was not accepted");
        return Err(totp_invalid().into());
    }
    taken.into_iter().for_each(AccountCheck::passed);
    let actor = client.actor(principal);
    local_totp::disable(&mut tx, scope, user_id, attempt.now, actor)
        .await
        .map_err(totp_error)?;
    tx.commit().await.map_err(internal)?;
    tracing::info!(%user_id, "TOTP disabled");
    Ok(StatusCode::NO_CONTENT.into_response())
}

fn attempt<'a>(state: &'a AppState, scope: OrgScope, headers: &'a HeaderMap) -> Attempt<'a> {
    Attempt {
        state,
        scope,
        headers,
        now: state.clock.now(),
    }
}

/// Lock the signed-in user, then their login, and read their TOTP. A user without a password
/// is 409 `local.password_required`.
async fn lock_own(
    tx: &mut Transaction<'_, Postgres>,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<Totp, ApiError> {
    identities::lock_user(tx, scope, user_id)
        .await
        .map_err(internal)?;
    local_totp::lock(tx, scope, user_id)
        .await
        .map_err(internal)?
        .filter(|totp| totp.has_password)
        .ok_or_else(|| totp_error(local_totp::Error::NoPassword))
}

/// The budgets a signed-in user's code check counts against: their own, and their account's or
/// their device cookie's, as for a change of password (B17, B18).
async fn own_budgets(
    conn: &mut PgConnection,
    limited: &RateLimited<SignIn>,
    attempt: &Attempt<'_>,
    user_id: Uuid,
) -> Result<Vec<AccountCheck>, Refusal> {
    let login = local_logins::get(&mut *conn, attempt.scope, user_id)
        .await
        .map_err(internal)?;
    let subject = Subject {
        email: login.as_ref().and_then(|login| login.email.as_ref()),
        login: login.as_ref(),
        signed_in: Some(user_id),
    };
    Ok(take_budgets(limited, attempt, &subject)?)
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// The answer for a TOTP repository error.
fn totp_error(error: local_totp::Error) -> ApiError {
    let (code, detail) = match error {
        local_totp::Error::NoPassword => (
            code::LOCAL_PASSWORD_REQUIRED,
            "Set a password before turning on TOTP: the code is asked for after the password.",
        ),
        local_totp::Error::AlreadyConfirmed => (
            code::LOCAL_TOTP_ENABLED,
            "TOTP is already on. Turn it off first to set up another app.",
        ),
        local_totp::Error::NotStarted => (
            code::LOCAL_TOTP_NOT_STARTED,
            "No TOTP setup is waiting for a code. Start the setup again.",
        ),
        local_totp::Error::NotConfirmed => (
            code::LOCAL_TOTP_NOT_ENABLED,
            "TOTP is not on, so there is nothing to turn off.",
        ),
        other => return internal(other),
    };
    ApiError::new(StatusCode::CONFLICT, code, detail)
}

/// 401 `auth.totp_invalid`.
fn totp_invalid() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        code::AUTH_TOTP_INVALID,
        "The code is wrong or was already used. Enter the current code from your app, or a recovery code.",
    )
}

/// 400 `auth.flow_invalid` for a flow whose account, password or TOTP changed since the
/// password was checked.
fn flow_changed() -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        flow::code::AUTH_FLOW_INVALID,
        SECOND_STEP_FLOW.invalid,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_have_their_status_and_code() {
        let invalid = totp_invalid().problem();
        assert_eq!(
            (invalid.status, invalid.code.as_str()),
            (401, "auth.totp_invalid")
        );
        let changed = flow_changed().problem();
        assert_eq!(
            (changed.status, changed.code.as_str()),
            (400, "auth.flow_invalid")
        );
        for (error, want) in [
            (local_totp::Error::NoPassword, "local.password_required"),
            (local_totp::Error::AlreadyConfirmed, "local.totp_enabled"),
            (local_totp::Error::NotStarted, "local.totp_not_started"),
            (local_totp::Error::NotConfirmed, "local.totp_not_enabled"),
        ] {
            let problem = totp_error(error).problem();
            assert_eq!((problem.status, problem.code.as_str()), (409, want));
        }
    }

    #[expect(clippy::disallowed_methods, reason = "a unit test without a request")]
    fn scope() -> OrgScope {
        OrgScope::instance(Uuid::from_u128(1))
    }

    #[test]
    fn a_sealed_secret_opens_only_for_its_user() {
        let keys = KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=").unwrap();
        let scope = scope();
        let (sam, ada) = (Uuid::from_u128(2), Uuid::from_u128(3));
        let secret = Secret::generate().unwrap();
        let sealed = seal_secret(&keys, scope, sam, &secret).unwrap();
        assert_ne!(sealed.as_slice(), secret.expose_bytes().as_slice());
        let opened = open_secret(&keys, scope, sam, &sealed).unwrap();
        assert_eq!(opened.expose_bytes(), secret.expose_bytes());
        assert!(open_secret(&keys, scope, ada, &sealed).is_err());
    }

    #[test]
    fn the_next_step_is_spelled_totp() {
        let body = serde_json::to_value(CodeRequired {
            next: NextStep::Totp,
        })
        .unwrap();
        assert_eq!(body, serde_json::json!({ "next": "totp" }));
    }
}
