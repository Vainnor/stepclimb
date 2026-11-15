// SPDX-License-Identifier: AGPL-3.0-or-later
//! The service account routes under `/api/v1/admin/service-accounts`
//! (docs/specs/m0/08-sessions-and-csrf.md B24, B28, B29, API, Permissions, Q9).
//!
//! A service account is how a machine (a script, a BI tool, another system) calls the API: it
//! holds one API key, sent as `Authorization: Bearer sc_...` ([`crate::api_key`]). Every route
//! takes `Require<ApiKeys>` (the plan's `api_keys` feature, spec 10) and then
//! `Require<ServiceAccountManage>` at organization scope, in that order, so a plan without the
//! feature answers 403 `entitlement.feature_unavailable` before the permission check and before
//! the id is looked up.
//!
//! Creating one makes the caller its owner (Q9 (a): every account has an owner) and generates
//! its key, which the 201 answer carries once; the server keeps only the key's SHA-256 and no
//! route returns the key again. Only a browser session creates one: the route takes `NoBearer`
//! first, so an API key cannot mint a key (403 `auth.session_required`, spec 08 B44). Each scope
//! must be a catalog permission key, listed once (422), that the caller holds at some scope (403
//! `service_account.scope_exceeds_owner`): a key's rights on each request are its scopes
//! intersected with its owner's current grants, so a scope the owner lacks would grant nothing
//! and is refused rather than stored. Deleting the account, which a key with
//! `service_account.manage` may do, makes its key fail on the next request. Creating and
//! deleting write their audit entries in the change's transaction.

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use stepclimb_auth::token::ApiKey;
use stepclimb_core::{
    feature::ApiKeys,
    grants::Grants,
    id::Uuid,
    perm::ServiceAccountManage,
    service_accounts::{self as repo, ScopeProblem},
};
use utoipa::{IntoParams, ToSchema};

use crate::{
    AppState,
    audit::ClientAddress,
    error::{ApiError, FieldError},
    extract::{ApiJson, ApiPath, ApiQuery},
    openapi::problem_responses,
    pagination::{Page, PageParams},
    require::{NoBearer, Require},
    roles::code::{NAME_EMPTY, NAME_TOO_LONG, TEXT_CONTAINS_NUL},
    timestamp,
};

/// The path of the service accounts collection; one is at
/// `{SERVICE_ACCOUNTS_PATH}/{service_account_id}`.
pub const SERVICE_ACCOUNTS_PATH: &str = "/api/v1/admin/service-accounts";

/// The codes this module produces.
pub mod code {
    /// No service account of the organization has this id (404).
    pub const SERVICE_ACCOUNT_NOT_FOUND: &str = "service_account.not_found";
    /// A scope names a permission the caller does not hold at any scope (403).
    pub const SCOPE_EXCEEDS_OWNER: &str = "service_account.scope_exceeds_owner";
    /// In a 422's `errors`: the scope is not a key of the permission catalog.
    pub const SCOPE_UNKNOWN: &str = "service_account.scope_unknown";
    /// In a 422's `errors`: the scope repeats an earlier entry of the list.
    pub const SCOPE_DUPLICATE: &str = "service_account.scope_duplicate";
}

problem_responses! {
    ServiceAccountsForbidden => "`entitlement.feature_unavailable`: the organization's plan does not include API keys. `permission.denied`: the caller does not hold `service_account.manage` at organization scope.",
    ServiceAccountCreateForbidden => "`auth.session_required`: the request carries a bearer credential; only a signed-in browser session creates API keys. `entitlement.feature_unavailable`: the organization's plan does not include API keys. `permission.denied`: the caller does not hold `service_account.manage` at organization scope. `service_account.scope_exceeds_owner`: a scope names a permission the caller does not hold. `auth.csrf_refused`: the request did not come from this site.",
    ServiceAccountDeleteForbidden => "`entitlement.feature_unavailable`: the organization's plan does not include API keys. `permission.denied`: the caller does not hold `service_account.manage` at organization scope. `auth.csrf_refused`: the request did not come from this site.",
    ServiceAccountNotFound => "`service_account.not_found`: no service account of the organization has this id. `org.not_found`: the request's organization cannot be determined.",
}

/// The kind of a service account.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ServiceAccountKind {
    /// It holds one API key.
    ApiKey,
}

impl From<repo::Kind> for ServiceAccountKind {
    fn from(kind: repo::Kind) -> Self {
        match kind {
            repo::Kind::ApiKey => Self::ApiKey,
        }
    }
}

/// A service account. Its key is never part of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ServiceAccount {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub name: String,
    pub kind: ServiceAccountKind,
    /// The permission keys the key may use, where its owner holds them.
    pub scopes: Vec<String>,
    /// The user whose current permissions the scopes are narrowed to.
    #[schema(value_type = String, format = "uuid")]
    pub owner_user_id: Uuid,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
    /// RFC 3339, UTC: the last request the key authenticated, to the minute; `null` before the
    /// first.
    #[schema(format = DateTime, required = true)]
    pub last_used_at: Option<String>,
}

impl ServiceAccount {
    fn from_repo(account: repo::ServiceAccount) -> Result<Self, ApiError> {
        let time = |t| timestamp::rfc3339(t).map_err(|error| ApiError::internal(&error));
        Ok(Self {
            created_at: time(account.created_at)?,
            last_used_at: account.last_used_at.map(time).transpose()?,
            id: account.id,
            name: account.name,
            kind: account.kind.into(),
            scopes: account.scopes,
            owner_user_id: account.owner_user_id,
        })
    }
}

/// A service account just created, with its API key. Its `Debug` output leaves the key out.
#[derive(Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CreatedServiceAccount {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub name: String,
    pub kind: ServiceAccountKind,
    pub scopes: Vec<String>,
    #[schema(value_type = String, format = "uuid")]
    pub owner_user_id: Uuid,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
    /// Always `null`: the key has not been used.
    #[schema(format = DateTime, required = true)]
    pub last_used_at: Option<String>,
    /// The API key, `sc_<random>_<checksum>`. This is the only answer that carries it: store it
    /// now. Send it as `Authorization: Bearer <key>`.
    pub key: String,
}

impl std::fmt::Debug for CreatedServiceAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreatedServiceAccount")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// A service account to create.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewServiceAccount {
    /// Trimmed; 1 to 200 characters.
    pub name: String,
    /// Catalog permission keys (`GET /api/v1/admin/permissions`), each at most once and each
    /// held by the caller at some scope.
    pub scopes: Vec<String>,
}

/// The path parameter of a single service account's routes.
#[derive(Clone, Copy, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Path)]
pub struct ServiceAccountIdParam {
    /// The service account's id.
    #[param(value_type = String, format = "uuid")]
    pub service_account_id: Uuid,
}

/// List the organization's service accounts, oldest first. Keys are never listed.
#[utoipa::path(
    get,
    path = "/api/v1/admin/service-accounts",
    operation_id = "listServiceAccounts",
    tag = "service-accounts",
    params(PageParams),
    responses(
        (status = 200, description = "One page of service accounts", body = Page<ServiceAccount>),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ServiceAccountsForbidden),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_service_accounts(
    _: Require<ApiKeys>,
    require: Require<ServiceAccountManage>,
    State(state): State<AppState>,
    ApiQuery(params): ApiQuery<PageParams>,
) -> Result<Json<Page<ServiceAccount>>, ApiError> {
    let page = params.parse::<repo::ListKey>()?;
    let rows = repo::list(
        &state.db,
        require.principal().scope(),
        page.after(),
        page.fetch_limit(),
    )
    .await
    .map_err(service_account_error)?;
    let rows = rows
        .into_iter()
        .map(ServiceAccount::from_repo)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(page.page(rows, |account| account.id)?))
}

/// Create a service account owned by the caller, and its API key.
#[utoipa::path(
    post,
    path = "/api/v1/admin/service-accounts",
    operation_id = "createServiceAccount",
    tag = "service-accounts",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = NewServiceAccount,
    responses(
        (status = 201, description = "The service account is created; `key` is shown this once", body = CreatedServiceAccount),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ServiceAccountCreateForbidden),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn create_service_account(
    _: NoBearer,
    _: Require<ApiKeys>,
    require: Require<ServiceAccountManage>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<NewServiceAccount>,
) -> Result<Response, ApiError> {
    refuse_exceeding_scopes(require.grants(), &body.scopes)?;
    let principal = require.principal();
    let key = ApiKey::generate().map_err(|error| ApiError::internal(&error))?;
    let new = repo::NewServiceAccount {
        name: body.name,
        owner_user_id: principal.user_id,
        scopes: body.scopes,
    };
    let account = repo::create(
        &state.db,
        principal.scope(),
        &new,
        key.hash().as_bytes(),
        state.clock.now(),
        client.actor(principal),
    )
    .await
    .map_err(service_account_error)?;
    let account = ServiceAccount::from_repo(account)?;
    let created = CreatedServiceAccount {
        id: account.id,
        name: account.name,
        kind: account.kind,
        scopes: account.scopes,
        owner_user_id: account.owner_user_id,
        created_at: account.created_at,
        last_used_at: account.last_used_at,
        key: key.expose_secret().to_owned(),
    };
    Ok((StatusCode::CREATED, Json(created)).into_response())
}

/// Delete a service account; its key stops working on the next request.
#[utoipa::path(
    delete,
    path = "/api/v1/admin/service-accounts/{service_account_id}",
    operation_id = "deleteServiceAccount",
    tag = "service-accounts",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(ServiceAccountIdParam),
    responses(
        (status = 204, description = "The service account and its key are deleted"),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ServiceAccountDeleteForbidden),
        (status = 404, response = ServiceAccountNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn delete_service_account(
    _: Require<ApiKeys>,
    require: Require<ServiceAccountManage>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiPath(ServiceAccountIdParam { service_account_id }): ApiPath<ServiceAccountIdParam>,
) -> Result<StatusCode, ApiError> {
    let principal = require.principal();
    repo::delete(
        &state.db,
        principal.scope(),
        service_account_id,
        state.clock.now(),
        client.actor(principal),
    )
    .await
    .map_err(service_account_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 403 `service_account.scope_exceeds_owner` when a catalog key in `scopes` is not effective for
/// the caller at any scope. A key outside the catalog is left to the repository's 422.
fn refuse_exceeding_scopes(grants: &Grants, scopes: &[String]) -> Result<(), ApiError> {
    let held = |scope: &str| grants.by_scope().any(|(_, keys)| keys.contains(&scope));
    let in_catalog = |scope: &str| repo::check_scopes(&[scope]).is_ok();
    if scopes.iter().all(|s| !in_catalog(s) || held(s)) {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::FORBIDDEN,
        code::SCOPE_EXCEEDS_OWNER,
        "A service account's scopes can only name permissions you hold.",
    ))
}

fn service_account_not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        code::SERVICE_ACCOUNT_NOT_FOUND,
        "No service account with this id exists in this organization.",
    )
}

/// The answer for a service accounts repository error.
fn service_account_error(error: repo::Error) -> ApiError {
    use repo::Error as E;
    let invalid = |path: &[&str], code| ApiError::invalid(vec![FieldError::at(path, code)]);
    match error {
        E::NotFound => service_account_not_found(),
        E::NameEmpty => invalid(&["name"], NAME_EMPTY),
        E::NameTooLong => invalid(&["name"], NAME_TOO_LONG),
        E::NameContainsNul => invalid(&["name"], TEXT_CONTAINS_NUL),
        E::Scope { index, problem } => invalid(
            &["scopes", &index.to_string()],
            match problem {
                ScopeProblem::Unknown => code::SCOPE_UNKNOWN,
                ScopeProblem::Duplicate => code::SCOPE_DUPLICATE,
            },
        ),
        // The owner is the authenticated caller, found within the organization this request.
        other @ (E::OwnerNotFound | E::Stored { .. } | E::Time(_) | E::Database(_)) => {
            ApiError::internal(&other)
        }
    }
}

#[cfg(test)]
mod tests {
    use stepclimb_core::{
        OrgScope,
        id::new_id,
        role_assignments::{GrantScope, UserGrant},
    };

    use super::*;

    #[expect(
        clippy::disallowed_methods,
        reason = "unit tests of the scope rule need grants and have no credential"
    )]
    fn grants(org: &[&str], account: &[&str]) -> Grants {
        let grant = |keys: &[&str], scope| UserGrant {
            assignment_id: new_id(),
            role_id: new_id(),
            role_key: "custom".into(),
            permissions: keys.iter().map(|&k| k.to_owned()).collect(),
            scope,
        };
        let account_id = new_id();
        Grants::from_assignments(
            OrgScope::instance(new_id()),
            [
                grant(org, GrantScope::Organization),
                grant(
                    account,
                    GrantScope::Account {
                        id: account_id,
                        path: vec![account_id],
                    },
                ),
            ],
        )
    }

    fn scopes(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|&k| k.to_owned()).collect()
    }

    #[test]
    fn a_scope_must_be_held_somewhere_by_the_caller() {
        let g = grants(&["service_account.manage"], &["account.manage"]);
        assert!(refuse_exceeding_scopes(&g, &scopes(&[])).is_ok());
        // Held at an account only: the key's rights stay at that account by the intersection.
        assert!(refuse_exceeding_scopes(&g, &scopes(&["account.manage"])).is_ok());
        let refused =
            refuse_exceeding_scopes(&g, &scopes(&["account.manage", "user.manage"])).unwrap_err();
        assert_eq!(refused.status(), StatusCode::FORBIDDEN);
        assert_eq!(refused.code(), code::SCOPE_EXCEEDS_OWNER);
        // A key outside the catalog is the repository's 422, not this 403.
        assert!(refuse_exceeding_scopes(&g, &scopes(&["reports:read"])).is_ok());
    }

    #[test]
    fn a_created_account_debug_output_leaves_the_key_out() {
        let key = ApiKey::generate().unwrap();
        let created = CreatedServiceAccount {
            id: new_id(),
            name: "Nightly export".into(),
            kind: ServiceAccountKind::ApiKey,
            scopes: Vec::new(),
            owner_user_id: new_id(),
            created_at: String::new(),
            last_used_at: None,
            key: key.expose_secret().to_owned(),
        };
        let printed = format!("{created:?}");
        assert!(!printed.contains(key.expose_secret()), "{printed}");
    }
}
