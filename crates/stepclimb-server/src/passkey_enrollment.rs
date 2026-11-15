// SPDX-License-Identifier: AGPL-3.0-or-later
//! Registering a passkey through an enrollment link (docs/specs/m0/04-local-accounts.md B5, B7,
//! B8, Q12; docs/specs/m0/08-sessions-and-csrf.md B17 to B20).
//!
//! The ceremony takes two requests. `POST /auth/local/enrollment/start`, when the organization
//! offers passkeys, calls [`begin`]: it builds the organization's relying party (its ID is the
//! public URL's host, its name the organization's), asks `stepclimb_auth::passkey` for creation
//! options that exclude the user's existing passkeys, and starts a `passkey_register` sign-in
//! flow for the link's user whose sealed payload holds the ceremony state and the link's id. The
//! browser gets the options and the flow cookie; the state stays on the server.
//!
//! `POST /auth/local/enrollment/passkey` ([`enroll_passkey`]) finishes it. It is rate limited
//! first and answers 404 `auth.method_disabled` while passkeys are off. In one transaction it
//! takes the flow (deleting it, so the challenge is used once), refuses one that holds no link
//! (a registration `POST /api/v1/me/passkeys/start` began, `crate::my_passkeys`) as 400
//! `auth.flow_invalid` with the deletion committed, then, in a savepoint:
//!
//! 1. locks the flow's user (`identities::lock_user`, which also refuses any isolation level
//!    but `READ COMMITTED`, the level `local_tokens::consume_enrollment` relies on);
//! 2. uses the link (`consume_enrollment`: unknown, used, replaced, unbacked or another
//!    organization's is 400 `auth.enrollment_invalid`, expired 400 `auth.enrollment_expired`);
//! 3. refuses a flow started for another link or user as 400 `auth.flow_invalid`;
//! 4. verifies the browser's response against the flow's state (challenge, origin, relying party
//!    ID, user verification): a failure is 400 `auth.passkey_invalid`;
//! 5. stores the passkey (`passkey.create`; a credential id the organization already has is
//!    `auth.passkey_invalid` too);
//! 6. links the local identity if the user has none (`identity.link`, subject the user's id,
//!    spec 04 Q4): only this route creates one;
//! 7. completes the sign-in through admission's local entry (`admission::complete_local`),
//!    whose step 1 refuses a suspended user with 403 `auth.suspended` and otherwise creates the
//!    session (spec 08 B7).
//!
//! A refusal at steps 2 to 7 rolls the savepoint back, so the link stays usable and nothing is
//! stored, and commits the flow's deletion. Every answer after the flow is taken clears the flow
//! cookie. No step logs the token, the flow token or the ceremony state.

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
use stepclimb_auth::{
    passkey::{self as webauthn, Registrant, RelyingParty},
    token::TokenHash,
};
use stepclimb_core::{
    OrgScope,
    audit::Actor,
    auth_flows::Kind,
    id::Uuid,
    identities::{self, NewIdentity},
    identity_providers,
    local_sign_in::{Method, PROVIDER_KEY},
    local_tokens::{self, Enrollment},
    organizations, passkeys, users,
};
use utoipa::ToSchema;

use crate::{
    AppState,
    admission::{self, Completion},
    audit::ClientAddress,
    enrollment::{enrollment_error, invalid_link},
    error::{ApiError, FieldError},
    extract::ApiJson,
    flow::{self, FinishError, FlowStart},
    local_sign_in::require_method,
    openapi::problem_responses,
    passkey_options::PasskeyCreationOptions,
    rate_limit::{RateLimited, SignIn},
    request_org::RequestOrg,
    roles::code::{NAME_EMPTY, NAME_TOO_LONG, TEXT_CONTAINS_NUL},
    session,
};

/// The route that finishes the ceremony.
pub const ENROLLMENT_PASSKEY_PATH: &str = "/auth/local/enrollment/passkey";

/// The name a passkey gets when the request names none.
pub const DEFAULT_NAME: &str = "Passkey";

/// The codes this module produces.
pub mod code {
    /// The registration response failed verification, or the authenticator is already
    /// registered (400).
    pub const AUTH_PASSKEY_INVALID: &str = "auth.passkey_invalid";
}

problem_responses! {
    EnrollPasskeyInvalid => "`request.malformed`: the body is not `{token, credential, name?}`. `auth.enrollment_invalid`: the link is unknown, already used, replaced by a newer link, or for another organization. `auth.enrollment_expired`: the link is older than its 7-day lifetime. `auth.flow_invalid`: no registration was started in this browser for this link, or it was already finished; open the link again. `auth.flow_expired`: the registration was started more than 10 minutes ago; open the link again. `auth.passkey_invalid`: the passkey's response did not verify, or this passkey is already registered; open the link again and retry.",
    EnrollPasskeyForbidden => "`auth.suspended`: the account is suspended. `auth.csrf_refused`: the request did not come from this site.",
    EnrollPasskeyNotFound => "`auth.method_disabled`: passkey sign-in is switched off for this organization. `org.not_found`: the request's organization cannot be determined.",
}

/// What the start route's passkey flow keeps, sealed: the link it was started for and the
/// ceremony state. Never logged.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    link_id: Uuid,
    state: String,
}

/// The passkey half of the start route's answer: the creation options for the browser, and the
/// flow cookie that holds the ceremony on the server.
pub struct Begun {
    pub options: PasskeyCreationOptions,
    /// The `Set-Cookie` value. It holds the flow token: never log it.
    pub set_cookie: HeaderValue,
}

/// The organization's relying party: the public URL's host as its ID, the organization's name
/// as its name.
pub(crate) async fn relying_party(
    state: &AppState,
    db: &mut PgConnection,
    scope: OrgScope,
) -> Result<RelyingParty, ApiError> {
    let org = organizations::get(&mut *db, scope)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    RelyingParty::new(state.public_url.as_str(), &org.name).map_err(|e| ApiError::internal(&e))
}

/// Start a passkey registration for the user of `enrollment` at `now` (B7), excluding their
/// existing passkeys, and the flow that keeps it.
pub async fn begin(
    state: &AppState,
    db: &mut PgConnection,
    scope: OrgScope,
    enrollment: &Enrollment,
    now: SystemTime,
) -> Result<Begun, ApiError> {
    let user = users::get(&mut *db, scope, enrollment.user_id)
        .await
        .map_err(|e| ApiError::internal(&e))?
        .ok_or_else(invalid_link)?;
    let existing = passkeys::credential_ids(&mut *db, scope, user.id)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let party = relying_party(state, db, scope).await?;
    let registrant = Registrant {
        user_id: user.id,
        name: &enrollment.email,
        display_name: &user.display_name,
    };
    let started = party
        .start_registration(registrant, existing)
        .map_err(|e| ApiError::internal(&e))?;
    let pending = Pending {
        link_id: enrollment.id,
        state: started.state,
    };
    let payload = serde_json::to_vec(&pending).map_err(|e| ApiError::internal(&e))?;
    let flow = FlowStart {
        kind: Kind::PasskeyRegister,
        user_id: Some(user.id),
        provider_id: None,
        payload: &payload,
        return_to: None,
    };
    let flow = flow::start(&mut *db, &state.keys, scope, flow, now)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let options =
        PasskeyCreationOptions::from_json(started.options).map_err(|e| ApiError::internal(&e))?;
    Ok(Begun {
        options,
        set_cookie: flow.set_cookie,
    })
}

/// The browser's registration response for an enrollment link.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PasskeyEnrollment {
    /// The text after `#token=` in the link.
    pub token: String,
    /// The `PublicKeyCredential` `navigator.credentials.create` returned, as JSON
    /// (`PublicKeyCredential.toJSON()`).
    #[schema(value_type = Object)]
    pub credential: Value,
    /// What the user calls this passkey: trimmed, 1 to 200 characters. `Passkey` when absent.
    pub name: Option<String>,
}

/// A completed sign-in.
#[derive(Debug, Serialize, ToSchema)]
pub struct SignedIn {
    /// The path on this site to open next.
    pub return_to: String,
}

/// Register a passkey through an enrollment link and sign in with it.
#[utoipa::path(
    post,
    path = "/auth/local/enrollment/passkey",
    operation_id = "enrollPasskey",
    tag = "auth",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = PasskeyEnrollment,
    responses(
        (
            status = 200,
            description = "The passkey is registered and the user is signed in",
            body = SignedIn,
            headers(("Set-Cookie" = String, description = "The new `__Host-sc_session` cookie, and `__Host-sc_flow` cleared"))
        ),
        (status = 400, response = EnrollPasskeyInvalid),
        (status = 403, response = EnrollPasskeyForbidden),
        (status = 404, response = EnrollPasskeyNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 429, response = crate::openapi::RateLimited),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn enroll_passkey(
    _limited: RateLimited<SignIn>,
    org: RequestOrg,
    client: ClientAddress,
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<PasskeyEnrollment>,
) -> Result<Response, ApiError> {
    let scope = org.scope();
    require_method(&state.db, scope, Method::Passkey).await?;
    let name = passkey_name(body.name.as_deref())?;
    let now = state.clock.now();
    let mut tx = state.db.begin().await.map_err(|e| ApiError::internal(&e))?;
    let flow = flow::finish(
        &mut *tx,
        &state.keys,
        scope,
        &headers,
        Kind::PasskeyRegister,
        now,
    )
    .await;
    let flow = match flow {
        Ok(flow) => flow,
        Err(error) => return flow_refused(tx, error, &REGISTRATION_FLOW).await,
    };
    // A `passkey_register` flow that `POST /api/v1/me/passkeys/start` began holds no link: it
    // is not this route's to finish.
    let Ok(pending) = serde_json::from_slice::<Pending>(&flow.payload) else {
        let refusal = with_flow_cleared(flow_mismatch().into_response());
        tx.commit().await.map_err(|e| ApiError::internal(&e))?;
        return Ok(refusal);
    };
    let attempt = Attempt {
        state: &state,
        scope,
        user_id: flow.user_id,
        pending: &pending,
        body: &body,
        name,
        actor_ip: client,
        headers: &headers,
        now,
    };
    let outcome = register(&mut tx, attempt).await;
    let response = match outcome {
        Ok(started) => signed_in(started, flow.return_to.as_str()),
        Err(Outcome::Refused(refusal)) => with_flow_cleared(refusal.into_response()),
        Err(Outcome::Failed(error)) => return Err(error),
    };
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    Ok(response)
}

/// The name to store: the request's, checked by the passkey name rule, or [`DEFAULT_NAME`].
/// Checked before the flow is taken, so a bad name does not use the challenge up.
pub(crate) fn passkey_name(name: Option<&str>) -> Result<String, ApiError> {
    let Some(name) = name else {
        return Ok(DEFAULT_NAME.to_owned());
    };
    let refused = |code| ApiError::invalid(vec![FieldError::at(&["name"], code)]);
    match passkeys::valid_name(name) {
        Ok(name) => Ok(name.to_owned()),
        Err(passkeys::Error::NameEmpty) => Err(refused(NAME_EMPTY)),
        Err(passkeys::Error::NameTooLong) => Err(refused(NAME_TOO_LONG)),
        Err(passkeys::Error::NameContainsNul) => Err(refused(TEXT_CONTAINS_NUL)),
        Err(other) => Err(ApiError::internal(&other)),
    }
}

/// What a passkey ceremony's flow refusals tell the user: when the flow expired, and when there
/// is none.
pub(crate) struct FlowDetails {
    pub expired: &'static str,
    pub invalid: &'static str,
}

const REGISTRATION_FLOW: FlowDetails = FlowDetails {
    expired: "This passkey registration started more than 10 minutes ago. Open the enrollment link again.",
    invalid: "No passkey registration is waiting in this browser. Open the enrollment link again.",
};

/// The answer for a flow [`flow::finish`] did not return: 400 `auth.flow_invalid` with nothing
/// changed, or 400 `auth.flow_expired` after committing the expired flow's deletion (spec 08
/// B21), with `details`' text and the flow cookie cleared.
pub(crate) async fn flow_refused(
    tx: Transaction<'_, Postgres>,
    error: FinishError,
    details: &FlowDetails,
) -> Result<Response, ApiError> {
    let Some(code) = error.code() else {
        return Err(ApiError::internal(&error));
    };
    if matches!(error, FinishError::Expired) {
        tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    }
    let detail = match error {
        FinishError::Expired => details.expired,
        _ => details.invalid,
    };
    let refusal = ApiError::new(StatusCode::BAD_REQUEST, code, detail);
    Ok(with_flow_cleared(refusal.into_response()))
}

/// What [`register`] needs besides the transaction.
struct Attempt<'a> {
    state: &'a AppState,
    scope: OrgScope,
    /// The user the flow was started for.
    user_id: Option<Uuid>,
    pending: &'a Pending,
    body: &'a PasskeyEnrollment,
    name: String,
    actor_ip: ClientAddress,
    headers: &'a HeaderMap,
    now: SystemTime,
}

/// Why [`register`] (or a passkey sign-in) did not sign the user in: a refusal the client
/// caused, answered after the flow's deletion commits, or a server fault, which rolls
/// everything back.
pub(crate) enum Outcome {
    Refused(ApiError),
    Failed(ApiError),
}

impl Outcome {
    pub(crate) fn internal(cause: &dyn std::error::Error) -> Self {
        Self::Failed(ApiError::internal(cause))
    }
}

impl From<Completion> for Outcome {
    /// An admission refusal answers its 403 problem details (spec 07 B2).
    fn from(completion: Completion) -> Self {
        match completion {
            Completion::Refused(refusal) => Self::Refused(refusal.into_error()),
            Completion::Failed(error) => Self::Failed(error),
        }
    }
}

/// Steps 1 to 7 of the module documentation, in a savepoint of `tx` that only a sign-in
/// releases.
async fn register(
    tx: &mut Transaction<'_, Postgres>,
    attempt: Attempt<'_>,
) -> Result<session::Started, Outcome> {
    let scope = attempt.scope;
    let mut sp = tx.begin().await.map_err(|e| Outcome::internal(&e))?;
    let Some(user_id) = attempt.user_id else {
        return Err(Outcome::Refused(flow_mismatch()));
    };
    let user = match identities::lock_user(&mut sp, scope, user_id).await {
        Ok(user) => user,
        Err(identities::Error::UserNotFound) => return Err(Outcome::Refused(invalid_link())),
        Err(other) => return Err(Outcome::internal(&other)),
    };
    let hash = TokenHash::of(&attempt.body.token);
    let enrollment =
        local_tokens::consume_enrollment(&mut sp, scope, hash.as_bytes(), attempt.now).await;
    let enrollment = match enrollment {
        Ok(enrollment) => enrollment,
        Err(error @ (local_tokens::Error::Invalid | local_tokens::Error::Expired)) => {
            return Err(Outcome::Refused(enrollment_error(error)));
        }
        Err(other) => return Err(Outcome::internal(&other)),
    };
    if enrollment.id != attempt.pending.link_id || enrollment.user_id != user.id {
        return Err(Outcome::Refused(flow_mismatch()));
    }
    let party = relying_party(attempt.state, &mut sp, scope)
        .await
        .map_err(Outcome::Failed)?;
    let registered = party
        .finish_registration(&attempt.body.credential, &attempt.pending.state)
        .map_err(verification_refused)?;
    let actor = Actor::User {
        id: user.id,
        ip: attempt.actor_ip.0,
    };
    let new = passkeys::NewPasskey {
        user_id: user.id,
        credential_id: &registered.credential_id,
        credential: &registered.credential,
        name: &attempt.name,
    };
    match passkeys::create(&mut *sp, scope, new, attempt.now, actor).await {
        Ok(_) => {}
        Err(passkeys::Error::CredentialTaken) => {
            tracing::info!(%user_id, "passkey enrollment refused: the credential is already registered");
            return Err(Outcome::Refused(passkey_invalid()));
        }
        Err(other) => return Err(Outcome::internal(&other)),
    }
    link_local_identity(&mut sp, scope, user.id, attempt.now, actor).await?;
    let started =
        admission::complete_local(&mut sp, scope, user.id, attempt.now, attempt.headers).await?;
    sp.commit().await.map_err(|e| Outcome::internal(&e))?;
    Ok(started)
}

/// Link the user to the organization's local provider with their id as the subject (spec 04 B3,
/// Q4), unless they already are.
async fn link_local_identity(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
    now: SystemTime,
    actor: Actor,
) -> Result<(), Outcome> {
    let provider = identity_providers::get_by_key(&mut *conn, scope, PROVIDER_KEY)
        .await
        .map_err(|e| Outcome::internal(&e))?
        .ok_or_else(|| Outcome::Failed(missing_local_row()))?;
    let subject = user_id.to_string();
    let linked = identities::find_user(&mut *conn, scope, provider.id, &subject)
        .await
        .map_err(|e| Outcome::internal(&e))?;
    if linked.is_some() {
        return Ok(());
    }
    let identity = NewIdentity {
        user_id,
        provider_id: provider.id,
        subject,
        attributes: serde_json::Map::new(),
    };
    identities::link(&mut *conn, scope, &identity, now, actor)
        .await
        .map_err(|e| Outcome::internal(&e))?;
    Ok(())
}

/// A missing local row is a server fault: `require_method` found it enabled moments ago, and
/// every organization has one from startup.
fn missing_local_row() -> ApiError {
    let cause = std::io::Error::other("the organization has no local identity provider row");
    ApiError::internal(&cause)
}

/// A failed verification, logged without the response or the state: the client's mistake or
/// an attacker's, and a server fault only when the stored state does not parse.
fn verification_refused(error: webauthn::FinishError) -> Outcome {
    match error {
        webauthn::FinishError::Malformed(_) | webauthn::FinishError::Refused(_) => {
            tracing::info!(%error, "passkey enrollment refused");
            Outcome::Refused(passkey_invalid())
        }
        webauthn::FinishError::State(_) | webauthn::FinishError::Serialize(_) => {
            Outcome::internal(&error)
        }
    }
}

fn passkey_invalid() -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        code::AUTH_PASSKEY_INVALID,
        "The passkey could not be registered. Open the enrollment link again and retry, or use another passkey.",
    )
}

fn flow_mismatch() -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        flow::code::AUTH_FLOW_INVALID,
        "No passkey registration for this enrollment link is waiting in this browser. Open the enrollment link again.",
    )
}

/// `response` with the flow cookie cleared, since the flow is gone.
pub(crate) fn with_flow_cleared(mut response: Response) -> Response {
    response
        .headers_mut()
        .append(header::SET_COOKIE, flow::clear_cookie());
    response
}

/// 200 `{return_to}` with the session cookie, the flow cookie cleared and `no-store`.
pub(crate) fn signed_in(started: session::Started, return_to: &str) -> Response {
    let body = SignedIn {
        return_to: return_to.to_owned(),
    };
    let mut response = Json(body).into_response();
    let headers = response.headers_mut();
    headers.append(header::SET_COOKIE, started.set_cookie);
    headers.append(header::SET_COOKIE, flow::clear_cookie());
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_name_is_the_default_and_a_given_one_is_trimmed() {
        assert_eq!(passkey_name(None).unwrap(), "Passkey");
        assert_eq!(passkey_name(Some(" Phone ")).unwrap(), "Phone");
    }

    #[test]
    fn a_bad_name_is_refused_at_name() {
        for (name, code) in [("  ", "name.empty"), ("a\0b", "text.contains_nul")] {
            let problem = passkey_name(Some(name)).unwrap_err().problem();
            assert_eq!(
                (problem.status, problem.code.as_str()),
                (422, "request.invalid")
            );
            assert_eq!(problem.errors[0].code, code, "{name:?}");
        }
    }

    #[test]
    fn the_refusals_carry_their_codes() {
        let invalid = passkey_invalid().problem();
        assert_eq!(
            (invalid.status, invalid.code.as_str()),
            (400, "auth.passkey_invalid")
        );
        let flow = flow_mismatch().problem();
        assert_eq!(
            (flow.status, flow.code.as_str()),
            (400, "auth.flow_invalid")
        );
    }
}
