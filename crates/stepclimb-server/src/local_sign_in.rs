// SPDX-License-Identifier: AGPL-3.0-or-later
//! The local sign-in switches under `/api/v1/admin/local-sign-in`, and the check every local
//! method's route makes (docs/specs/m0/04-local-accounts.md B1, B2, API).
//!
//! Reading and changing the switches need `provider.manage` at organization scope (Q2). A `PUT`
//! names every switch and the local row's `enabled`. One that changes anything changes how every
//! user of the organization signs in, so it is refused with 403 `permission.escalation` unless
//! the caller holds every catalog permission at organization scope (the owner's rule for writes
//! that affect another user's sign-in). That is the rule of the user routes (a permission
//! effective for a user the write affects must be effective for the caller at organization
//! scope) applied to every user at once: an Organization admin holds every catalog permission,
//! so the two agree in any organization that has one, and with none this refuses more. The
//! check reads the switches under the organization row's lock, which every settings write takes,
//! so a `PUT` it passes as changing nothing cannot change something by the time it writes.
//!
//! The repository's `local_sign_in::update` refuses the rest under the same lock: the magic
//! link, which needs email (M1; 422 `local.magic_link_needs_email`), and a change that would
//! leave the organization no primary way to sign in (409 `local.last_method`, B2): no offered
//! passkey, password or magic link (TOTP is only a second step) and no enabled provider that is
//! not local. Disabling the local row counts as such a change.
//!
//! [`require_method`] is the check of B2: a route of a method the organization does not offer
//! answers 404 `auth.method_disabled`. The method routes of spec 04 issues 2 to 8 call it, or
//! [`require_any_method`] for a route of several methods, first.

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use sqlx::PgExecutor;
use stepclimb_core::{
    OrgScope,
    local_sign_in::{self as repo, LocalMethods, LocalSignIn as CoreLocalSignIn, Method},
    perm::ProviderManage,
};
use utoipa::ToSchema;

use crate::{
    AppState,
    audit::ClientAddress,
    error::{ApiError, FieldError},
    extract::ApiJson,
    openapi::problem_responses,
    request_org,
    require::Require,
    roles::refuse_unless_holds_every_permission,
};

/// The path of the local sign-in switches.
pub const LOCAL_SIGN_IN_PATH: &str = "/api/v1/admin/local-sign-in";

/// The codes this module produces.
pub mod code {
    /// The organization does not offer the local method the route belongs to (404).
    pub const AUTH_METHOD_DISABLED: &str = "auth.method_disabled";
    /// In a 422's `errors`: the magic link cannot be switched on before email exists (M1).
    pub const LOCAL_MAGIC_LINK_NEEDS_EMAIL: &str = "local.magic_link_needs_email";
    /// The change would leave the organization no primary way to sign in (409).
    pub const LOCAL_LAST_METHOD: &str = "local.last_method";
}

problem_responses! {
    LocalSignInForbidden => "`permission.denied`: the caller does not hold `provider.manage` at organization scope. `permission.escalation`: the change affects how every user signs in, and the caller does not hold every catalog permission at organization scope. `auth.csrf_refused`: the request did not come from this site.",
    LocalSignInConflict => "`local.last_method`: the change would leave the organization no way to sign in: no offered passkey, password or magic link (TOTP is only a second step), and no enabled outside provider.",
    LocalSignInInvalid => "`request.invalid`: the change breaks a rule; `errors` lists each one. `local.magic_link_needs_email` marks `magic_link` switched on while the server cannot send email.",
}

/// The organization's local sign-in: whether it is on at all, and each method's switch. A method
/// is offered only when `enabled` and its own switch are both on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalSignIn {
    /// Local sign-in as a whole; off, no local method is offered.
    pub enabled: bool,
    /// Sign-in with a passkey.
    pub passkey: bool,
    /// Sign-in with a password.
    pub password: bool,
    /// A TOTP code as the second step after a password.
    pub totp: bool,
    /// Sign-in with a link sent by email. Off until the server can send email.
    pub magic_link: bool,
}

impl From<CoreLocalSignIn> for LocalSignIn {
    fn from(local: CoreLocalSignIn) -> Self {
        let LocalMethods {
            passkey,
            password,
            totp,
            magic_link,
        } = local.methods;
        Self {
            enabled: local.enabled,
            passkey,
            password,
            totp,
            magic_link,
        }
    }
}

impl From<LocalSignIn> for CoreLocalSignIn {
    fn from(body: LocalSignIn) -> Self {
        Self {
            enabled: body.enabled,
            methods: LocalMethods {
                passkey: body.passkey,
                password: body.password,
                totp: body.totp,
                magic_link: body.magic_link,
            },
        }
    }
}

/// Read the organization's local sign-in switches.
#[utoipa::path(
    get,
    path = "/api/v1/admin/local-sign-in",
    operation_id = "getLocalSignIn",
    tag = "local-sign-in",
    responses(
        (status = 200, description = "The local sign-in switches", body = LocalSignIn),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_local_sign_in(
    require: Require<ProviderManage>,
    State(state): State<AppState>,
) -> Result<Json<LocalSignIn>, ApiError> {
    let local = repo::get(&state.db, require.principal().scope())
        .await
        .map_err(local_error)?;
    Ok(Json(local.into()))
}

/// Set every local sign-in switch. A body equal to the current switches changes nothing.
#[utoipa::path(
    put,
    path = "/api/v1/admin/local-sign-in",
    operation_id = "updateLocalSignIn",
    tag = "local-sign-in",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = LocalSignIn,
    responses(
        (status = 200, description = "The switches after the change", body = LocalSignIn),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = LocalSignInForbidden),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 409, response = LocalSignInConflict),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = LocalSignInInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_local_sign_in(
    require: Require<ProviderManage>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<LocalSignIn>,
) -> Result<Json<LocalSignIn>, ApiError> {
    let principal = require.principal();
    let scope = principal.scope();
    let new = CoreLocalSignIn::from(body);
    let mut tx = state.db.begin().await.map_err(|e| ApiError::internal(&e))?;
    let current = repo::get_for_update(&mut tx, scope)
        .await
        .map_err(local_error)?;
    if current == new {
        return Ok(Json(current.into()));
    }
    refuse_unless_holds_every_permission(
        require.grants(),
        "Local sign-in affects every user, including users with permissions you do not hold. Only someone holding every permission can change it.",
    )?;
    let actor = client.actor(principal);
    let after = repo::update(&mut *tx, scope, new, state.clock.now(), actor)
        .await
        .map_err(local_error)?;
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    Ok(Json(after.into()))
}

/// 404 `auth.method_disabled` unless the scope's organization offers `method`: its local row is
/// enabled and the method's switch is on (B2). Every route of a local method calls this before
/// it reads anything else, so a switched-off method's credentials stop working for sign-in and
/// work again, unchanged, once it is switched back on.
pub async fn require_method(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    method: Method,
) -> Result<(), ApiError> {
    require_any_method(db, scope, &[method]).await.map(|_| ())
}

/// [`require_method`] for a route that serves several methods, such as the enrollment link,
/// which can set up a passkey or a password: the methods of `methods` the organization offers,
/// in `methods`' order, or 404 `auth.method_disabled` when it offers none of them.
pub async fn require_any_method(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    methods: &[Method],
) -> Result<Vec<Method>, ApiError> {
    let local = repo::get(db, scope).await.map_err(local_error)?;
    let offered: Vec<Method> = methods
        .iter()
        .copied()
        .filter(|method| local.offers(*method))
        .collect();
    if offered.is_empty() {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            code::AUTH_METHOD_DISABLED,
            "This sign-in method is switched off for this organization.",
        ));
    }
    Ok(offered)
}

/// The answer for a local sign-in repository error.
fn local_error(error: repo::Error) -> ApiError {
    match error {
        repo::Error::MagicLinkNeedsEmail => ApiError::invalid(vec![FieldError::at(
            &["magic_link"],
            code::LOCAL_MAGIC_LINK_NEEDS_EMAIL,
        )]),
        repo::Error::LastMethod => ApiError::new(
            StatusCode::CONFLICT,
            code::LOCAL_LAST_METHOD,
            "This change would leave no way to sign in. Enable another sign-in method first.",
        ),
        repo::Error::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_org::code::ORG_NOT_FOUND,
            "The request's organization cannot be determined.",
        ),
        other => ApiError::internal(&other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_api_shape_round_trips_through_the_core_type() {
        let body = LocalSignIn {
            enabled: false,
            passkey: true,
            password: false,
            totp: true,
            magic_link: false,
        };
        assert_eq!(LocalSignIn::from(CoreLocalSignIn::from(body)), body);
    }

    #[test]
    fn a_body_must_name_every_switch_and_nothing_else() {
        let full = serde_json::json!({
            "enabled": true, "passkey": true, "password": true, "totp": true, "magic_link": false
        });
        assert!(serde_json::from_value::<LocalSignIn>(full.clone()).is_ok());
        let mut missing = full.clone();
        missing.as_object_mut().unwrap().remove("totp");
        assert!(serde_json::from_value::<LocalSignIn>(missing).is_err());
        let mut extra = full.clone();
        extra["sso"] = true.into();
        assert!(serde_json::from_value::<LocalSignIn>(extra).is_err());
        let mut null = full;
        null["passkey"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<LocalSignIn>(null).is_err());
    }

    #[test]
    fn repository_refusals_map_to_their_status_code_and_field() {
        let magic = local_error(repo::Error::MagicLinkNeedsEmail).problem();
        assert_eq!(
            (magic.status, magic.code.as_str()),
            (422, "request.invalid")
        );
        assert_eq!(magic.errors[0].field, "/magic_link");
        assert_eq!(magic.errors[0].code, "local.magic_link_needs_email");
        let last = local_error(repo::Error::LastMethod).problem();
        assert_eq!(
            (last.status, last.code.as_str()),
            (409, "local.last_method")
        );
        let missing = local_error(repo::Error::RowMissing).problem();
        assert_eq!(missing.status, 500);
    }
}
