// SPDX-License-Identifier: AGPL-3.0-or-later
//! The signed-in user's passkeys under `/api/v1/me/passkeys` (docs/specs/m0/04-local-accounts.md
//! B13, Q10, Permissions; docs/specs/m0/08-sessions-and-csrf.md B16 to B20).
//!
//! Every route takes [`SessionOnly`]: no credential answers 401, a bearer credential (an API key
//! or an app access token) 403 `auth.session_required`. Each then answers 404
//! `auth.method_disabled` while the organization does not offer passkeys (B2, the API section's
//! rule for every route of a method), and acts on the caller's own passkeys only: another user's
//! passkey id, in this organization or another, answers 404 `passkey.not_found`.
//!
//! Adding a passkey is B8's ceremony with the session's user in place of an enrollment link.
//! `POST /api/v1/me/passkeys/start` ([`start_my_passkey`]) answers creation options that exclude
//! the user's passkeys, bound to a new `passkey_register` flow whose sealed payload holds the
//! ceremony state. `POST /api/v1/me/passkeys` ([`create_my_passkey`]) takes the flow (deleting
//! it, so the challenge is used once), then, in a savepoint:
//!
//! 1. refuses a flow that holds a link (an enrollment's) or was started for another user, 400
//!    `auth.flow_invalid`;
//! 2. locks the user (`identities::lock_user`, the lock identity removals take);
//! 3. applies the provider rule below, 403 `local.passkeys_not_allowed`;
//! 4. verifies the browser's response against the flow's state: 400 `auth.passkey_invalid`;
//! 5. stores the passkey (`passkey.create`; a credential id the organization already has is
//!    `auth.passkey_invalid` too).
//!
//! A refusal rolls the savepoint back and commits the flow's deletion; every answer after the
//! flow is taken clears the flow cookie. Adding a passkey links no local identity: only an
//! enrollment link makes a local account.
//!
//! The provider rule (decided by the owner, 15 Nov 2026): a user may add a passkey when they
//! have an identity at the local provider (a local account, which only an enrollment link
//! creates), or an identity at an enabled outside provider whose config sets
//! `allow_local_passkeys` to `true`. The member is off unless set exactly to `true`; the `oidc`
//! config check (`stepclimb_auth::oidc_config`) validates it, and the `oauth2` one of
//! docs/specs/m0/06-oauth2-and-vatsim-preset.md has the same member. Start and add both apply
//! it, add under the user's lock.
//!
//! Renaming writes `passkey.rename`. Deleting writes `passkey.delete`, and is refused with 409
//! `local.last_method` when the passkey is the user's last way to sign in
//! (`local::delete_passkey_unless_last`, Q10). No step logs the flow token, the ceremony state
//! or the browser's response.

use std::time::SystemTime;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Acquire, PgConnection, Postgres, Transaction};
use stepclimb_auth::{
    oidc_config::ALLOW_LOCAL_PASSKEYS,
    passkey::{self as webauthn, Registrant},
};
use stepclimb_core::{
    OrgScope,
    audit::Actor,
    auth_flows::Kind,
    id::Uuid,
    identities,
    identity_providers::{self, IdentityProvider},
    local::{self, DeleteError},
    local_sign_in::Method,
    passkeys::{self, Passkey},
    users,
};
use utoipa::{IntoParams, ToSchema};

use crate::{
    AppState,
    audit::ClientAddress,
    error::ApiError,
    extract::{ApiJson, ApiPath},
    flow::{self, FlowStart},
    local_sign_in::{code::LOCAL_LAST_METHOD, require_method},
    openapi::problem_responses,
    passkey_enrollment::{
        Begun, FlowDetails, Outcome, flow_refused, passkey_name, relying_party, with_flow_cleared,
    },
    passkey_options::PasskeyCreationOptions,
    require::SessionOnly,
    timestamp,
};

/// The path of the signed-in user's passkeys; one is at `{MY_PASSKEYS_PATH}/{passkey_id}`.
pub const MY_PASSKEYS_PATH: &str = "/api/v1/me/passkeys";

/// The route that starts adding a passkey.
pub const MY_PASSKEYS_START_PATH: &str = "/api/v1/me/passkeys/start";

/// The path of one of the signed-in user's passkeys.
pub const MY_PASSKEY_PATH: &str = "/api/v1/me/passkeys/{passkey_id}";

/// The codes this module produces besides the shared ones.
pub mod code {
    /// The user has no passkey with this id (404).
    pub const PASSKEY_NOT_FOUND: &str = "passkey.not_found";
    /// No provider the user signs in with lets its users add a passkey, and the user has no
    /// local account (403).
    pub const LOCAL_PASSKEYS_NOT_ALLOWED: &str = "local.passkeys_not_allowed";
    /// The registration response failed verification, or the authenticator is already
    /// registered (400).
    pub const AUTH_PASSKEY_INVALID: &str = "auth.passkey_invalid";
}

problem_responses! {
    MyPasskeysForbidden => "`auth.session_required`: the request carries a bearer credential; only a signed-in browser session manages passkeys.",
    MyPasskeyWriteForbidden => "`auth.session_required`: the request carries a bearer credential; only a signed-in browser session manages passkeys. `auth.csrf_refused`: the request did not come from this site.",
    MyPasskeyAddForbidden => "`auth.session_required`: the request carries a bearer credential; only a signed-in browser session manages passkeys. `local.passkeys_not_allowed`: you have no local account, and no identity provider you sign in with lets its users add a passkey. `auth.csrf_refused`: the request did not come from this site.",
    MyPasskeysNotFound => "`auth.method_disabled`: passkey sign-in is switched off for this organization. `org.not_found`: the request's organization cannot be determined.",
    MyPasskeyNotFound => "`passkey.not_found`: you have no passkey with this id. `auth.method_disabled`: passkey sign-in is switched off for this organization. `org.not_found`: the request's organization cannot be determined.",
    MyPasskeyAddInvalid => "`request.malformed`: the body is not `{credential, name?}`. `auth.flow_invalid`: no passkey registration was started in this browser for you, or it was already finished; start again. `auth.flow_expired`: the registration was started more than 10 minutes ago; start again. `auth.passkey_invalid`: the passkey's response did not verify, or this passkey is already registered; start again and retry.",
    MyPasskeyDeleteConflict => "`local.last_method`: this passkey is your last way to sign in. Add another passkey, or sign in once with an identity provider, first.",
}

/// One of the signed-in user's passkeys. The credential itself never leaves the server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct MyPasskey {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub name: String,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
    /// The last sign-in with it, RFC 3339, UTC; `null` before the first.
    #[schema(format = DateTime, required = true)]
    pub last_used_at: Option<String>,
}

impl MyPasskey {
    fn from_repo(passkey: Passkey) -> Result<Self, ApiError> {
        let time = |t| timestamp::rfc3339(t).map_err(|error| ApiError::internal(&error));
        Ok(Self {
            created_at: time(passkey.created_at)?,
            last_used_at: passkey.last_used_at.map(time).transpose()?,
            id: passkey.id,
            name: passkey.name,
        })
    }
}

/// The creation options for the browser.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyPasskeyStarted {
    /// Pass `publicKey` to `navigator.credentials.create`, then send the result to `POST
    /// /api/v1/me/passkeys` in this browser within 10 minutes.
    pub options: PasskeyCreationOptions,
}

/// The browser's registration response.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewMyPasskey {
    /// The `PublicKeyCredential` `navigator.credentials.create` returned, as JSON
    /// (`PublicKeyCredential.toJSON()`).
    #[schema(value_type = Object)]
    pub credential: Value,
    /// What you call this passkey: trimmed, 1 to 200 characters. `Passkey` when absent.
    pub name: Option<String>,
}

/// A new name for a passkey.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MyPasskeyPatch {
    /// Trimmed, 1 to 200 characters.
    pub name: String,
}

/// The path parameter of one of the signed-in user's passkeys.
#[derive(Clone, Copy, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Path)]
pub struct PasskeyIdParam {
    /// The passkey's id.
    #[param(value_type = String, format = "uuid")]
    pub passkey_id: Uuid,
}

/// What the start route's flow keeps, sealed: the ceremony state. It holds no link, which tells
/// it apart from an enrollment's flow of the same kind. Never logged.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    state: String,
}

const ADD_FLOW: FlowDetails = FlowDetails {
    expired: "This passkey registration started more than 10 minutes ago. Start again.",
    invalid: "No passkey registration for you is waiting in this browser. Start again.",
};

/// List your passkeys, oldest first.
#[utoipa::path(
    get,
    path = "/api/v1/me/passkeys",
    operation_id = "listMyPasskeys",
    tag = "me",
    responses(
        (status = 200, description = "Your passkeys", body = Vec<MyPasskey>),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = MyPasskeysForbidden),
        (status = 404, response = MyPasskeysNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_my_passkeys(
    auth: SessionOnly,
    State(state): State<AppState>,
) -> Result<Json<Vec<MyPasskey>>, ApiError> {
    let principal = auth.principal();
    let scope = principal.scope();
    require_method(&state.db, scope, Method::Passkey).await?;
    let passkeys = passkeys::list_for_user(&state.db, scope, principal.user_id)
        .await
        .map_err(|error| ApiError::internal(&error))?;
    passkeys
        .into_iter()
        .map(MyPasskey::from_repo)
        .collect::<Result<_, _>>()
        .map(Json)
}

/// Start adding a passkey to your account.
#[utoipa::path(
    post,
    path = "/api/v1/me/passkeys/start",
    operation_id = "startMyPasskey",
    tag = "me",
    extensions(("x-stepclimb-mutates" = json!(true))),
    responses(
        (
            status = 200,
            description = "The creation options; the registration is waiting in this browser",
            body = MyPasskeyStarted,
            headers(("Set-Cookie" = String, description = "The `__Host-sc_flow` cookie of the registration"))
        ),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = MyPasskeyAddForbidden),
        (status = 404, response = MyPasskeysNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn start_my_passkey(
    auth: SessionOnly,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    let principal = auth.principal();
    let scope = principal.scope();
    require_method(&state.db, scope, Method::Passkey).await?;
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    refuse_unless_allowed(&mut conn, scope, principal.user_id).await?;
    let begun = begin(
        &state,
        &mut conn,
        scope,
        principal.user_id,
        state.clock.now(),
    )
    .await?;
    let mut response = Json(MyPasskeyStarted {
        options: begun.options,
    })
    .into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, begun.set_cookie);
    Ok(response)
}

/// Start a passkey registration for the user `user_id` at `now`, excluding their passkeys, and
/// the `passkey_register` flow that keeps it. The WebAuthn user name is the user's email, or
/// their display name when they have none. The caller has applied the provider rule.
pub async fn begin(
    state: &AppState,
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
    now: SystemTime,
) -> Result<Begun, ApiError> {
    let user = users::get(&mut *conn, scope, user_id)
        .await
        .map_err(|e| ApiError::internal(&e))?
        .ok_or_else(|| ApiError::internal(&OwnUserMissing))?;
    let existing = passkeys::credential_ids(&mut *conn, scope, user.id)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let party = relying_party(state, conn, scope).await?;
    let registrant = Registrant {
        user_id: user.id,
        name: user.email.as_deref().unwrap_or(&user.display_name),
        display_name: &user.display_name,
    };
    let started = party
        .start_registration(registrant, existing)
        .map_err(|e| ApiError::internal(&e))?;
    let payload = serde_json::to_vec(&Pending {
        state: started.state,
    })
    .map_err(|e| ApiError::internal(&e))?;
    let flow = FlowStart {
        kind: Kind::PasskeyRegister,
        user_id: Some(user.id),
        provider_id: None,
        payload: &payload,
        return_to: None,
    };
    let flow = flow::start(&mut *conn, &state.keys, scope, flow, now)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let options =
        PasskeyCreationOptions::from_json(started.options).map_err(|e| ApiError::internal(&e))?;
    Ok(Begun {
        options,
        set_cookie: flow.set_cookie,
    })
}

/// Add the passkey whose registration `POST /api/v1/me/passkeys/start` began in this browser.
#[utoipa::path(
    post,
    path = "/api/v1/me/passkeys",
    operation_id = "createMyPasskey",
    tag = "me",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = NewMyPasskey,
    responses(
        (
            status = 201,
            description = "The passkey is added",
            body = MyPasskey,
            headers(("Set-Cookie" = String, description = "`__Host-sc_flow` cleared"))
        ),
        (status = 400, response = MyPasskeyAddInvalid),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = MyPasskeyAddForbidden),
        (status = 404, response = MyPasskeysNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn create_my_passkey(
    auth: SessionOnly,
    client: ClientAddress,
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<NewMyPasskey>,
) -> Result<Response, ApiError> {
    let principal = auth.principal();
    let scope = principal.scope();
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
        Err(error) => return flow_refused(tx, error, &ADD_FLOW).await,
    };
    let attempt = Attempt {
        state: &state,
        scope,
        user_id: principal.user_id,
        flow: &flow,
        credential: &body.credential,
        name: &name,
        actor: client.actor(principal),
        now,
    };
    let response = match add(&mut tx, attempt).await {
        Ok(passkey) => {
            let created = (StatusCode::CREATED, Json(MyPasskey::from_repo(passkey)?));
            with_flow_cleared(created.into_response())
        }
        Err(Outcome::Refused(refusal)) => with_flow_cleared(refusal.into_response()),
        Err(Outcome::Failed(error)) => return Err(error),
    };
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    Ok(response)
}

/// What [`add`] needs besides the transaction.
struct Attempt<'a> {
    state: &'a AppState,
    scope: OrgScope,
    /// The signed-in user.
    user_id: Uuid,
    flow: &'a flow::Finished,
    credential: &'a Value,
    name: &'a str,
    actor: Actor,
    now: SystemTime,
}

/// Steps 1 to 5 of the module documentation, in a savepoint of `tx` that only a stored passkey
/// releases.
async fn add(tx: &mut Transaction<'_, Postgres>, attempt: Attempt<'_>) -> Result<Passkey, Outcome> {
    let scope = attempt.scope;
    let Ok(pending) = serde_json::from_slice::<Pending>(&attempt.flow.payload) else {
        return Err(Outcome::Refused(flow_mismatch()));
    };
    if attempt.flow.user_id != Some(attempt.user_id) {
        return Err(Outcome::Refused(flow_mismatch()));
    }
    let mut sp = tx.begin().await.map_err(|e| Outcome::internal(&e))?;
    identities::lock_user(&mut sp, scope, attempt.user_id)
        .await
        .map_err(|e| Outcome::internal(&e))?;
    match refuse_unless_allowed(&mut sp, scope, attempt.user_id).await {
        Ok(()) => {}
        Err(refusal) if refusal.status() == StatusCode::FORBIDDEN => {
            return Err(Outcome::Refused(refusal));
        }
        Err(error) => return Err(Outcome::Failed(error)),
    }
    let party = relying_party(attempt.state, &mut sp, scope)
        .await
        .map_err(Outcome::Failed)?;
    let registered = party
        .finish_registration(attempt.credential, &pending.state)
        .map_err(verification_refused)?;
    let new = passkeys::NewPasskey {
        user_id: attempt.user_id,
        credential_id: &registered.credential_id,
        credential: &registered.credential,
        name: attempt.name,
    };
    let passkey = match passkeys::create(&mut *sp, scope, new, attempt.now, attempt.actor).await {
        Ok(passkey) => passkey,
        Err(passkeys::Error::CredentialTaken) => {
            let user_id = attempt.user_id;
            tracing::info!(%user_id, "adding a passkey refused: the credential is already registered");
            return Err(Outcome::Refused(passkey_invalid()));
        }
        Err(other) => return Err(Outcome::internal(&other)),
    };
    sp.commit().await.map_err(|e| Outcome::internal(&e))?;
    Ok(passkey)
}

/// Rename one of your passkeys.
#[utoipa::path(
    patch,
    path = "/api/v1/me/passkeys/{passkey_id}",
    operation_id = "updateMyPasskey",
    tag = "me",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(PasskeyIdParam),
    request_body = MyPasskeyPatch,
    responses(
        (status = 200, description = "The passkey with its new name", body = MyPasskey),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = MyPasskeyWriteForbidden),
        (status = 404, response = MyPasskeyNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_my_passkey(
    auth: SessionOnly,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiPath(PasskeyIdParam { passkey_id }): ApiPath<PasskeyIdParam>,
    ApiJson(body): ApiJson<MyPasskeyPatch>,
) -> Result<Json<MyPasskey>, ApiError> {
    let principal = auth.principal();
    let scope = principal.scope();
    require_method(&state.db, scope, Method::Passkey).await?;
    let name = passkey_name(Some(&body.name))?;
    let actor = client.actor(principal);
    let now = state.clock.now();
    let renamed = passkeys::rename(
        &state.db,
        scope,
        principal.user_id,
        passkey_id,
        &name,
        now,
        actor,
    )
    .await
    .map_err(|error| match error {
        passkeys::Error::NotFound => passkey_not_found(),
        other => ApiError::internal(&other),
    })?;
    MyPasskey::from_repo(renamed).map(Json)
}

/// Delete one of your passkeys. You must keep a way to sign in.
#[utoipa::path(
    delete,
    path = "/api/v1/me/passkeys/{passkey_id}",
    operation_id = "deleteMyPasskey",
    tag = "me",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(PasskeyIdParam),
    responses(
        (status = 204, description = "The passkey is deleted"),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = MyPasskeyWriteForbidden),
        (status = 404, response = MyPasskeyNotFound),
        (status = 409, response = MyPasskeyDeleteConflict),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn delete_my_passkey(
    auth: SessionOnly,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiPath(PasskeyIdParam { passkey_id }): ApiPath<PasskeyIdParam>,
) -> Result<StatusCode, ApiError> {
    let principal = auth.principal();
    let scope = principal.scope();
    require_method(&state.db, scope, Method::Passkey).await?;
    let actor = client.actor(principal);
    let now = state.clock.now();
    local::delete_passkey_unless_last(&state.db, scope, principal.user_id, passkey_id, now, actor)
        .await
        .map_err(delete_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// The answer for a refused or failed deletion.
fn delete_error(error: DeleteError) -> ApiError {
    match error {
        DeleteError::NotFound => passkey_not_found(),
        DeleteError::LastSignInMethod => ApiError::new(
            StatusCode::CONFLICT,
            LOCAL_LAST_METHOD,
            "This passkey is your last way to sign in. Add another passkey first.",
        ),
        other => ApiError::internal(&other),
    }
}

/// 403 `local.passkeys_not_allowed` unless the provider rule of the module documentation lets
/// the user `user_id` add a passkey.
async fn refuse_unless_allowed(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<(), ApiError> {
    let providers = identity_providers::list_for_user(&mut *conn, scope, user_id)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    if may_add_passkey(&providers) {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::FORBIDDEN,
        code::LOCAL_PASSKEYS_NOT_ALLOWED,
        "You sign in through an identity provider that does not let its users add a passkey.",
    ))
}

/// Whether a user with identities at `providers` may add a passkey: one of them is the local
/// provider, or an enabled outside provider whose config sets `allow_local_passkeys` to `true`.
/// A user with no identity may not.
fn may_add_passkey(providers: &[IdentityProvider]) -> bool {
    providers.iter().any(|provider| match provider.kind {
        identity_providers::Kind::Local => true,
        identity_providers::Kind::Oidc | identity_providers::Kind::Oauth2 => {
            provider.enabled
                && provider.config.get(ALLOW_LOCAL_PASSKEYS) == Some(&Value::Bool(true))
        }
    })
}

/// A failed verification, logged without the response or the state, as the enrollment's is.
fn verification_refused(error: webauthn::FinishError) -> Outcome {
    match error {
        webauthn::FinishError::Malformed(_) | webauthn::FinishError::Refused(_) => {
            tracing::info!(%error, "adding a passkey refused");
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
        "The passkey could not be added. Start again and retry, or use another passkey.",
    )
}

fn flow_mismatch() -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        flow::code::AUTH_FLOW_INVALID,
        ADD_FLOW.invalid,
    )
}

fn passkey_not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        code::PASSKEY_NOT_FOUND,
        "You have no passkey with this id.",
    )
}

/// The signed-in user is not in their session's organization.
#[derive(Debug, thiserror::Error)]
#[error("the authenticated user is not in its organization")]
struct OwnUserMissing;

#[cfg(test)]
mod tests {
    use serde_json::{Map, json};

    use super::*;

    fn provider(kind: identity_providers::Kind, enabled: bool, config: Value) -> IdentityProvider {
        let Value::Object(config) = config else {
            panic!("a config object");
        };
        IdentityProvider {
            id: Uuid::nil(),
            key: "p".into(),
            kind,
            config,
            enabled,
            created_at: SystemTime::UNIX_EPOCH,
            updated_at: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn an_outside_provider_allows_passkeys_only_when_enabled_and_set_to_true() {
        use identity_providers::Kind::{Oauth2, Oidc};
        let on = json!({ "allow_local_passkeys": true });
        assert!(may_add_passkey(&[provider(Oidc, true, on.clone())]));
        assert!(may_add_passkey(&[provider(Oauth2, true, on.clone())]));
        assert!(!may_add_passkey(&[provider(Oidc, false, on)]));
        for off in [
            json!({}),
            json!({ "allow_local_passkeys": false }),
            json!({ "allow_local_passkeys": "true" }),
            json!({ "allow_local_passkeys": 1 }),
        ] {
            assert!(!may_add_passkey(&[provider(Oidc, true, off)]));
        }
    }

    #[test]
    fn a_local_account_allows_passkeys_and_no_identity_does_not() {
        use identity_providers::Kind::{Local, Oidc};
        let local = provider(Local, true, Value::Object(Map::new()));
        let closed = provider(Oidc, true, json!({}));
        assert!(may_add_passkey(&[closed.clone(), local]));
        assert!(!may_add_passkey(&[closed]));
        assert!(!may_add_passkey(&[]));
    }

    #[test]
    fn the_refusals_carry_their_codes() {
        let cases = [
            (passkey_invalid(), 400, "auth.passkey_invalid"),
            (flow_mismatch(), 400, "auth.flow_invalid"),
            (passkey_not_found(), 404, "passkey.not_found"),
            (
                delete_error(DeleteError::NotFound),
                404,
                "passkey.not_found",
            ),
            (
                delete_error(DeleteError::LastSignInMethod),
                409,
                "local.last_method",
            ),
        ];
        for (error, status, code) in cases {
            let problem = error.problem();
            assert_eq!((problem.status, problem.code.as_str()), (status, code));
        }
    }
}
