// SPDX-License-Identifier: AGPL-3.0-or-later
//! The identity routes: an admin's under `/api/v1/users/{user_id}/identities` and the signed-in
//! user's under `/api/v1/me/identities` (docs/specs/m0/03-users-and-identities.md B18, B19,
//! B23 and API).
//!
//! An identity links a user to an identity provider by the provider's `subject`; signing in
//! through that provider with that subject signs in as the user. The admin routes take the
//! organization as their target: reading needs `user.view` and writing `user.manage` at
//! organization scope. A user id of another organization is 404 `user.not_found`, as an id that
//! does not exist, and an identity id of another user or organization is 404
//! `identity.not_found`.
//!
//! Both admin writes change how the user signs in, so both apply the user routes' escalation rule
//! (`users::refuse_escalation`, 403 `permission.escalation`): a user with an effective permission,
//! at any scope, that the caller does not hold at organization scope is refused. Each runs in one
//! transaction that takes the locks in the order the user routes take them: the organization's
//! account tree lock, which role assignment writes take too; then, inside the escalation check, the
//! user's custom roles `FOR KEY SHARE`; then the user's row, which `identities::unlink_unless_last`
//! locks `FOR NO KEY UPDATE`. Until the write commits, no assignment or custom role change can
//! alter what the check read. A malformed subject is refused before any lock. Linking refuses an
//! unknown provider key (422 `identity.unknown_provider`, the same for another organization's key)
//! and a `(provider, subject)` already linked to anyone (409 `identity.taken`). Removing refuses
//! the user's last sign-in method (409 `identity.last_sign_in_method`); only an identity at an
//! enabled provider counts as one. Neither route links or removes an identity at the local
//! provider (422 `identity.unknown_provider`, 409 `identity.local`): a local account changes
//! through docs/specs/m0/04-local-accounts.md's routes.
//!
//! The `/me` routes need a signed-in user and no permission, and act on the caller's own
//! identities only: another user's identity id is 404. Removing one changes how the user signs
//! in, so it takes `SessionOnly`: an API key, whatever its scopes, answers 403
//! `auth.session_required` there, as on the passkey routes. A user whose record an outside source
//! owns cannot remove their identities (409 `identity.externally_managed`), since the source
//! would re-create them. `/me/identities` never answers `attributes`, which can hold personal
//! data the provider sent.
//!
//! Each of the three writes records who made it: `identities::link` and `identities::unlink`
//! write the `identity.link` and `identity.unlink` audit entries (spec 09 B22) in the write's
//! transaction, with the caller and the client address ([`ClientAddress`]).

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::{Postgres, Transaction};
use stepclimb_core::{
    Source, accounts,
    grants::Grants,
    id::Uuid,
    identities::{self as repo, NewIdentity as RepoNewIdentity},
    identity_providers,
    perm::{UserManage, UserView},
    users as users_repo,
};
use utoipa::{IntoParams, ToSchema};

use crate::{
    AppState,
    audit::ClientAddress,
    error::{ApiError, FieldError},
    extract::{ApiJson, ApiPath},
    openapi::problem_responses,
    require::{AuthOnly, Require, SessionOnly},
    roles::code::TEXT_CONTAINS_NUL,
    session::Authenticated,
    timestamp,
    users::{
        UserIdParam, UserNotFound,
        code::{IDENTITY_SUBJECT_EMPTY, IDENTITY_TAKEN, IDENTITY_UNKNOWN_PROVIDER, TEXT_TOO_LONG},
        refuse_escalation, user_not_found,
    },
};

/// The path of the signed-in user's identities; one is at `{MY_IDENTITIES_PATH}/{identity_id}`.
pub const MY_IDENTITIES_PATH: &str = "/api/v1/me/identities";

/// The codes this module produces besides those of `users::code` and `user.not_found`.
pub mod code {
    /// No identity with this id belongs to the user (404).
    pub const IDENTITY_NOT_FOUND: &str = "identity.not_found";
    /// Removing the identity would leave the user no way to sign in (409).
    pub const IDENTITY_LAST_SIGN_IN_METHOD: &str = "identity.last_sign_in_method";
    /// The identity is a local account's, which spec 04's routes change, not these (409).
    pub const IDENTITY_LOCAL: &str = "identity.local";
    /// An outside source owns the user, so the user cannot remove their identities (409).
    pub const IDENTITY_EXTERNALLY_MANAGED: &str = "identity.externally_managed";
}

/// The `permission.escalation` detail of both admin writes.
const ESCALATION_DETAIL: &str =
    "This user holds permissions you do not. You cannot change how they sign in.";

problem_responses! {
    IdentityWriteForbidden => "`permission.denied`: the caller does not hold `user.manage` at organization scope. `permission.escalation`: the user holds a permission, at any scope, that the caller does not hold at organization scope. `auth.session_required`: an API key acts on its own owner, or an app token on its own user. `auth.csrf_refused`: the request did not come from this site.",
    IdentityLinkConflict => "`identity.taken`: this provider's subject is already linked to a user.",
    IdentityDeleteNotFound => "`user.not_found`: no user of the organization has this id. `identity.not_found`: the user has no identity with this id. `org.not_found`: the request's organization cannot be determined.",
    IdentityDeleteConflict => "`identity.last_sign_in_method`: the user would be left with no way to sign in. `identity.local`: the identity is the user's local account, which is not removed here.",
    MyIdentityNotFound => "`identity.not_found`: you have no identity with this id. `org.not_found`: the request's organization cannot be determined.",
    MyIdentityDeleteForbidden => "`auth.session_required`: the request carries a bearer credential; only a signed-in browser session removes your identities. `auth.csrf_refused`: the request did not come from this site.",
    MyIdentityDeleteConflict => "`identity.last_sign_in_method`: you would be left with no way to sign in. `identity.local`: the identity is your local account, which is not removed here. `identity.externally_managed`: an identity provider, the sync API, SCIM or a connector owns your user record and would link the identity again; ask an admin.",
}

/// A user's identity as an admin sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Identity {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    /// The key of the identity provider.
    pub provider_key: String,
    /// The provider's id for the person, exactly as the provider sends it.
    pub subject: String,
    /// The attributes the provider sent at the latest sign-in, as mapped by the provider's
    /// configuration; `{}` before the first sign-in. Admission rules read them.
    #[schema(value_type = Object)]
    pub attributes: Map<String, Value>,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub updated_at: String,
}

impl Identity {
    fn from_repo(identity: repo::Identity) -> Result<Self, ApiError> {
        let time = |t| timestamp::rfc3339(t).map_err(|error| ApiError::internal(&error));
        Ok(Self {
            created_at: time(identity.created_at)?,
            updated_at: time(identity.updated_at)?,
            id: identity.id,
            provider_key: identity.provider_key,
            subject: identity.subject,
            attributes: identity.attributes,
        })
    }
}

/// One of the signed-in user's identities.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct MyIdentity {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    /// The key of the identity provider.
    pub provider_key: String,
    /// The provider's label, such as "Sign in with Example"; `null` when the provider's
    /// configuration has none.
    #[schema(required = true)]
    pub provider_label: Option<String>,
    /// The provider's id for you, exactly as the provider sends it.
    pub subject: String,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
}

impl MyIdentity {
    fn from_repo(identity: repo::Identity) -> Result<Self, ApiError> {
        Ok(Self {
            created_at: timestamp::rfc3339(identity.created_at)
                .map_err(|error| ApiError::internal(&error))?,
            id: identity.id,
            provider_key: identity.provider_key,
            provider_label: identity.provider_label,
            subject: identity.subject,
        })
    }
}

/// An identity to link to a user, so they can sign in with it the first time.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewIdentity {
    /// The key of one of the organization's identity providers.
    pub provider_key: String,
    /// The provider's id for the person; stored exactly as given, not empty.
    pub subject: String,
}

/// The path parameters of one of a user's identities.
#[derive(Clone, Copy, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Path)]
pub struct UserIdentityParams {
    /// The user's id.
    #[param(value_type = String, format = "uuid")]
    pub user_id: Uuid,
    /// The identity's id.
    #[param(value_type = String, format = "uuid")]
    pub identity_id: Uuid,
}

/// The path parameter of one of the signed-in user's identities.
#[derive(Clone, Copy, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Path)]
pub struct IdentityIdParam {
    /// The identity's id.
    #[param(value_type = String, format = "uuid")]
    pub identity_id: Uuid,
}

/// List a user's identities, oldest first.
#[utoipa::path(
    get,
    path = "/api/v1/users/{user_id}/identities",
    operation_id = "listUserIdentities",
    tag = "users",
    params(UserIdParam),
    responses(
        (status = 200, description = "The user's identities", body = Vec<Identity>),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = UserNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_user_identities(
    require: Require<UserView>,
    State(state): State<AppState>,
    ApiPath(UserIdParam { user_id }): ApiPath<UserIdParam>,
) -> Result<Json<Vec<Identity>>, ApiError> {
    let scope = require.principal().scope();
    users_repo::get(&state.db, scope, user_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(user_not_found)?;
    let identities = repo::list_for_user(&state.db, scope, user_id)
        .await
        .map_err(|error| ApiError::internal(&error))?;
    identities
        .into_iter()
        .map(Identity::from_repo)
        .collect::<Result<_, _>>()
        .map(Json)
}

/// Link an identity to a user, so they can sign in with it.
#[utoipa::path(
    post,
    path = "/api/v1/users/{user_id}/identities",
    operation_id = "createUserIdentity",
    tag = "users",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(UserIdParam),
    request_body = NewIdentity,
    responses(
        (status = 201, description = "The identity is linked", body = Identity),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = IdentityWriteForbidden),
        (status = 404, response = UserNotFound),
        (status = 409, response = IdentityLinkConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn create_user_identity(
    require: Require<UserManage>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiPath(UserIdParam { user_id }): ApiPath<UserIdParam>,
    ApiJson(body): ApiJson<NewIdentity>,
) -> Result<Response, ApiError> {
    repo::validate_subject(&body.subject).map_err(identity_error)?;
    let mut tx = begin_on_user(&state, require.principal(), require.grants(), user_id).await?;
    let scope = require.principal().scope();
    let provider = identity_providers::get_by_key(&mut *tx, scope, &body.provider_key)
        .await
        .map_err(|error| match error {
            identity_providers::Error::ContainsNul { .. } => {
                invalid("provider_key", TEXT_CONTAINS_NUL)
            }
            other => ApiError::internal(&other),
        })?
        // The local provider takes no pre-provisioned identity: a local identity's subject is
        // its user's id, and enrollment creates it (spec 04 B3, Q4).
        .filter(|provider| provider.kind != identity_providers::Kind::Local)
        .ok_or_else(|| invalid("provider_key", IDENTITY_UNKNOWN_PROVIDER))?;
    let new = RepoNewIdentity {
        user_id,
        provider_id: provider.id,
        subject: body.subject,
        attributes: Map::new(),
    };
    let actor = client.actor(require.principal());
    let linked = repo::link(&mut *tx, scope, &new, state.clock.now(), actor)
        .await
        .map_err(identity_error)?;
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    let linked = Identity::from_repo(linked)?;
    Ok((StatusCode::CREATED, Json(linked)).into_response())
}

/// Remove an identity from a user. The user must keep a way to sign in.
#[utoipa::path(
    delete,
    path = "/api/v1/users/{user_id}/identities/{identity_id}",
    operation_id = "deleteUserIdentity",
    tag = "users",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(UserIdentityParams),
    responses(
        (status = 204, description = "The identity is removed"),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = IdentityWriteForbidden),
        (status = 404, response = IdentityDeleteNotFound),
        (status = 409, response = IdentityDeleteConflict),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn delete_user_identity(
    require: Require<UserManage>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiPath(UserIdentityParams {
        user_id,
        identity_id,
    }): ApiPath<UserIdentityParams>,
) -> Result<StatusCode, ApiError> {
    let mut tx = begin_on_user(&state, require.principal(), require.grants(), user_id).await?;
    let scope = require.principal().scope();
    let actor = client.actor(require.principal());
    let now = state.clock.now();
    repo::unlink_unless_last(&mut *tx, scope, user_id, identity_id, now, actor)
        .await
        .map_err(identity_error)?;
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

/// A transaction holding the account tree lock and the user's custom roles `FOR KEY SHARE`,
/// once the escalation rule has passed for the user `user_id`: 404 `user.not_found` when the
/// organization has no such user, 403 `permission.escalation` when they hold a permission the
/// caller lacks, 403 `auth.session_required` when an API key acts on its own owner or an app token on its own user.
/// `crate::enrollment` takes it too: an enrollment link changes how the user signs
/// in.
pub(crate) async fn begin_on_user(
    state: &AppState,
    principal: Authenticated,
    grants: &Grants,
    user_id: Uuid,
) -> Result<Transaction<'static, Postgres>, ApiError> {
    let scope = grants.scope();
    let mut tx = state.db.begin().await.map_err(|e| ApiError::internal(&e))?;
    // Before the user's grants are read: an assignment created or removed meanwhile holds this
    // lock until it commits, so the check sees the grants the write applies to.
    accounts::lock_tree(&mut tx, scope)
        .await
        .map_err(|error| ApiError::internal(&error))?;
    users_repo::get(&mut *tx, scope, user_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(user_not_found)?;
    refuse_escalation(&mut tx, principal, grants, user_id, ESCALATION_DETAIL).await?;
    Ok(tx)
}

/// List the signed-in user's identities, oldest first, without the attributes providers sent.
#[utoipa::path(
    get,
    path = "/api/v1/me/identities",
    operation_id = "listMyIdentities",
    tag = "me",
    responses(
        (status = 200, description = "Your identities", body = Vec<MyIdentity>),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_my_identities(
    auth: AuthOnly,
    State(state): State<AppState>,
) -> Result<Json<Vec<MyIdentity>>, ApiError> {
    let principal = auth.principal();
    let identities = repo::list_for_user(&state.db, principal.scope(), principal.user_id)
        .await
        .map_err(|error| ApiError::internal(&error))?;
    identities
        .into_iter()
        .map(MyIdentity::from_repo)
        .collect::<Result<_, _>>()
        .map(Json)
}

/// Remove one of the signed-in user's identities. You must keep a way to sign in.
#[utoipa::path(
    delete,
    path = "/api/v1/me/identities/{identity_id}",
    operation_id = "deleteMyIdentity",
    tag = "me",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(IdentityIdParam),
    responses(
        (status = 204, description = "The identity is removed"),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = MyIdentityDeleteForbidden),
        (status = 404, response = MyIdentityNotFound),
        (status = 409, response = MyIdentityDeleteConflict),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn delete_my_identity(
    auth: SessionOnly,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiPath(IdentityIdParam { identity_id }): ApiPath<IdentityIdParam>,
) -> Result<StatusCode, ApiError> {
    let principal = auth.principal();
    let scope = principal.scope();
    let mut tx = state.db.begin().await.map_err(|e| ApiError::internal(&e))?;
    let user = repo::lock_user(&mut tx, scope, principal.user_id)
        .await
        .map_err(own_user_error)?;
    if user.source != Source::Local {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            code::IDENTITY_EXTERNALLY_MANAGED,
            "An outside system owns your user record and would link this identity again. Ask an admin.",
        ));
    }
    let actor = client.actor(principal);
    let now = state.clock.now();
    repo::unlink_unless_last(&mut *tx, scope, principal.user_id, identity_id, now, actor)
        .await
        .map_err(own_user_error)?;
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

/// [`identity_error`], except that the caller's own user missing is a broken invariant (500):
/// the session was found by joining its user within the organization, and users are never
/// deleted in M0.
fn own_user_error(error: repo::Error) -> ApiError {
    match error {
        repo::Error::UserNotFound => ApiError::internal(&OwnUserMissing),
        other => identity_error(other),
    }
}

/// The signed-in user is not in their session's organization.
#[derive(Debug, thiserror::Error)]
#[error("the authenticated user is not in its organization")]
struct OwnUserMissing;

/// 422 `request.invalid` with `code` at the body member `member`.
fn invalid(member: &str, code: &'static str) -> ApiError {
    ApiError::invalid(vec![FieldError::at(&[member], code)])
}

/// The answer for an identities repository error.
fn identity_error(error: repo::Error) -> ApiError {
    use repo::Error as E;
    let conflict = |code, detail| ApiError::new(StatusCode::CONFLICT, code, detail);
    match error {
        E::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            code::IDENTITY_NOT_FOUND,
            "The user has no identity with this id.",
        ),
        E::UserNotFound => user_not_found(),
        E::Taken => conflict(IDENTITY_TAKEN, "This identity is already linked to a user."),
        E::LocalIdentity => conflict(
            code::IDENTITY_LOCAL,
            "This identity is a local account. Change local sign-in through its own settings.",
        ),
        E::LastSignInMethod => conflict(
            code::IDENTITY_LAST_SIGN_IN_METHOD,
            "This is the user's last way to sign in. Link another identity first.",
        ),
        E::SubjectEmpty => invalid("subject", IDENTITY_SUBJECT_EMPTY),
        E::SubjectTooLong => invalid("subject", TEXT_TOO_LONG),
        E::ContainsNul { .. } => invalid("subject", TEXT_CONTAINS_NUL),
        other @ (E::ProviderNotFound
        | E::IsolationLevel { .. }
        | E::StoredAttributes
        | E::User(_)
        | E::Local(_)
        | E::Time(_)
        | E::Database(_)) => ApiError::internal(&other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_errors_map_to_their_status_code_and_field() {
        let cases = [
            (repo::Error::NotFound, 404, "identity.not_found", None),
            (repo::Error::UserNotFound, 404, "user.not_found", None),
            (repo::Error::Taken, 409, "identity.taken", None),
            (
                repo::Error::LastSignInMethod,
                409,
                "identity.last_sign_in_method",
                None,
            ),
            (repo::Error::LocalIdentity, 409, "identity.local", None),
            (
                repo::Error::SubjectEmpty,
                422,
                "request.invalid",
                Some(("/subject", "identity.subject_empty")),
            ),
            (
                repo::Error::SubjectTooLong,
                422,
                "request.invalid",
                Some(("/subject", "text.too_long")),
            ),
            (
                repo::Error::ContainsNul { field: "subject" },
                422,
                "request.invalid",
                Some(("/subject", "text.contains_nul")),
            ),
            // The provider was read in the same transaction, so its absence is a bug.
            (repo::Error::ProviderNotFound, 500, "internal.error", None),
            (
                repo::Error::IsolationLevel {
                    found: "serializable".into(),
                },
                500,
                "internal.error",
                None,
            ),
        ];
        for (error, status, code, field) in cases {
            let problem = identity_error(error).problem();
            assert_eq!((problem.status, problem.code.as_str()), (status, code));
            let fields: Vec<_> = problem
                .errors
                .iter()
                .map(|e| (e.field.as_str(), e.code.as_str()))
                .collect();
            assert_eq!(fields, field.into_iter().collect::<Vec<_>>(), "{code}");
        }
    }
}
