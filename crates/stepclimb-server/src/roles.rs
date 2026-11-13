// SPDX-License-Identifier: AGPL-3.0-or-later
//! The permission catalog and roles routes under `/api/v1/admin`
//! (docs/specs/m0/09-permissions-and-roles.md B7 to B10 and API).
//!
//! Reading the catalog and the roles needs `role.view` at any scope: a role is an
//! organization-wide definition, so an account admin sees every role it may assign. Writing a
//! custom role needs `role.manage`, which is grantable only at organization scope, and the
//! escalation rule (B8, Q4): the caller must hold, at organization scope, every permission the
//! role gains (all of a new role's), and on a change also every permission it loses, so
//! `role.manage` alone neither mints a role stronger than its holder nor strips one of rights
//! its holder lacks. A permission the change keeps is not checked. The roles
//! repository (`stepclimb_core::roles`) checks the role itself: key format, unique key, name,
//! catalog keys, built-in roles immutable, no delete while assigned.

use std::collections::BTreeSet;

use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use stepclimb_core::{
    grants::{Grants, Target},
    id::Uuid,
    perm::{self, RoleManage, RoleView},
    roles,
};
use utoipa::{IntoParams, ToSchema};

use crate::{
    AppState,
    error::{ApiError, FieldError},
    extract::{ApiJson, ApiPath, ApiQuery},
    openapi::problem_responses,
    pagination::{Page, PageParams},
    require::{AnyScope, Require},
    timestamp,
};

/// The path of the roles collection; a role is at `{ROLES_PATH}/{role_id}`.
pub const ROLES_PATH: &str = "/api/v1/admin/roles";

/// The codes this module produces.
pub mod code {
    /// No built-in role and no custom role of the organization has this id (404).
    pub const ROLE_NOT_FOUND: &str = "role.not_found";
    /// A built-in role or another custom role of the organization has the key (409).
    pub const ROLE_KEY_TAKEN: &str = "role.key_taken";
    /// Built-in roles cannot be changed or deleted (409).
    pub const ROLE_BUILTIN_IMMUTABLE: &str = "role.builtin_immutable";
    /// The custom role still has assignments (409).
    pub const ROLE_IN_USE: &str = "role.in_use";
    /// The caller does not hold, at organization scope, a permission the role gains or loses
    /// (403).
    pub const PERMISSION_ESCALATION: &str = "permission.escalation";
    /// In a 422's `errors`: the key is not `^[a-z][a-z0-9_]{0,62}$`.
    pub const ROLE_KEY_INVALID: &str = "role.key_invalid";
    /// In a 422's `errors`: the permission is not in the catalog.
    pub const ROLE_UNKNOWN_PERMISSION: &str = "role.unknown_permission";
    /// In a 422's `errors`: the permission repeats an earlier entry of the list.
    pub const ROLE_DUPLICATE_PERMISSION: &str = "role.duplicate_permission";
    /// In a 422's `errors`: the name is empty once trimmed.
    pub const NAME_EMPTY: &str = "name.empty";
    /// In a 422's `errors`: the name is longer than 200 characters once trimmed.
    pub const NAME_TOO_LONG: &str = "name.too_long";
    /// In a 422's `errors`: the string holds the NUL character, which cannot be stored.
    pub const TEXT_CONTAINS_NUL: &str = "text.contains_nul";
}

problem_responses! {
    RoleNotFound => "`role.not_found`: no built-in role and no custom role of the organization has this id. `org.not_found`: the request's organization cannot be determined.",
    WriteForbidden => "`permission.denied`: the caller does not hold `role.manage` at organization scope. `permission.escalation`: the caller does not hold, at organization scope, a permission the role gains or loses. `auth.csrf_refused`: the request did not come from this site.",
    DeleteForbidden => "`permission.denied`: the caller does not hold `role.manage` at organization scope. `auth.csrf_refused`: the request did not come from this site.",
    CreateConflict => "`role.key_taken`: a built-in role or another custom role of the organization has the key.",
    UpdateConflict => "`role.builtin_immutable`: built-in roles cannot be changed.",
    DeleteConflict => "`role.builtin_immutable`: built-in roles cannot be deleted. `role.in_use`: the role still has assignments; remove them first.",
}

/// The kind of scope a permission can take effect at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ScopeType {
    Organization,
    Account,
    Course,
}

impl From<perm::ScopeType> for ScopeType {
    fn from(scope: perm::ScopeType) -> Self {
        match scope {
            perm::ScopeType::Organization => Self::Organization,
            perm::ScopeType::Account => Self::Account,
            perm::ScopeType::Course => Self::Course,
        }
    }
}

/// One permission of the catalog.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct PermissionInfo {
    /// The key roles list, `area.action`.
    pub key: String,
    /// The scope types at which a role assignment grants it.
    pub grantable: Vec<ScopeType>,
}

/// A built-in role or a custom role of the organization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Role {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub key: String,
    pub name: String,
    /// Built-in roles are the same in every organization and cannot be changed or deleted.
    pub builtin: bool,
    /// The permission keys the role grants. A built-in role's are this server's list for it.
    pub permissions: Vec<String>,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub updated_at: String,
}

impl Role {
    fn from_repo(role: roles::Role) -> Result<Self, ApiError> {
        let time = |t| timestamp::rfc3339(t).map_err(|error| ApiError::internal(&error));
        Ok(Self {
            created_at: time(role.created_at)?,
            updated_at: time(role.updated_at)?,
            id: role.id,
            key: role.key,
            name: role.name,
            builtin: role.builtin,
            permissions: role.permissions,
        })
    }
}

/// A custom role to create.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewRole {
    /// A lowercase letter, then up to 62 lowercase letters, digits or underscores; not the key
    /// of a built-in role or another custom role of the organization.
    pub key: String,
    /// Trimmed; 1 to 200 characters.
    pub name: String,
    /// Catalog keys (`GET /api/v1/admin/permissions`), each at most once and each held by the
    /// caller at organization scope.
    pub permissions: Vec<String>,
}

/// A change to a custom role; an omitted member is left as it is.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RolePatch {
    /// Trimmed; 1 to 200 characters.
    #[serde(default)]
    pub name: Option<String>,
    /// The role's whole new list, each catalog key at most once. The caller must hold at
    /// organization scope every key it adds and every key it drops.
    #[serde(default)]
    pub permissions: Option<Vec<String>>,
}

/// The path parameter of a single role's routes.
#[derive(Clone, Copy, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Path)]
pub struct RoleIdParam {
    /// The role's id.
    #[param(value_type = String, format = "uuid")]
    pub role_id: Uuid,
}

/// List the permission catalog.
#[utoipa::path(
    get,
    path = "/api/v1/admin/permissions",
    operation_id = "listPermissions",
    tag = "roles",
    responses(
        (status = 200, description = "Every permission, in catalog order", body = Vec<PermissionInfo>),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_permissions(_: Require<RoleView, AnyScope>) -> Json<Vec<PermissionInfo>> {
    Json(
        perm::ALL
            .iter()
            .map(|p| PermissionInfo {
                key: p.key().to_owned(),
                grantable: p.grantable().iter().copied().map(ScopeType::from).collect(),
            })
            .collect(),
    )
}

/// List the roles: the built-in roles, then the organization's custom roles, each by key.
#[utoipa::path(
    get,
    path = "/api/v1/admin/roles",
    operation_id = "listRoles",
    tag = "roles",
    params(PageParams),
    responses(
        (status = 200, description = "One page of roles", body = Page<Role>),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_roles(
    require: Require<RoleView, AnyScope>,
    State(state): State<AppState>,
    ApiQuery(params): ApiQuery<PageParams>,
) -> Result<Json<Page<Role>>, ApiError> {
    let page = params.parse::<roles::ListKey>()?;
    let rows = roles::list(
        &state.db,
        require.principal().scope(),
        page.after(),
        page.fetch_limit(),
    )
    .await
    .map_err(|error| match error {
        // Only the cursor's key is a caller's string here.
        roles::Error::ContainsNul { .. } => ApiError::invalid_cursor(),
        other => role_error(other),
    })?;
    let rows = rows
        .into_iter()
        .map(Role::from_repo)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(
        page.page(rows, |role| (!role.builtin, role.key.clone()))?,
    ))
}

/// Read one role.
#[utoipa::path(
    get,
    path = "/api/v1/admin/roles/{role_id}",
    operation_id = "getRole",
    tag = "roles",
    params(RoleIdParam),
    responses(
        (status = 200, description = "The role", body = Role),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = RoleNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_role(
    require: Require<RoleView, AnyScope>,
    State(state): State<AppState>,
    ApiPath(RoleIdParam { role_id }): ApiPath<RoleIdParam>,
) -> Result<Json<Role>, ApiError> {
    let role = roles::get(&state.db, require.principal().scope(), role_id)
        .await
        .map_err(role_error)?
        .ok_or_else(role_not_found)?;
    Ok(Json(Role::from_repo(role)?))
}

/// Create a custom role.
#[utoipa::path(
    post,
    path = "/api/v1/admin/roles",
    operation_id = "createRole",
    tag = "roles",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = NewRole,
    responses(
        (
            status = 201,
            description = "The role is created",
            body = Role,
            headers(("Location" = String, description = "The role's path"))
        ),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = WriteForbidden),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 409, response = CreateConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn create_role(
    require: Require<RoleManage>,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<NewRole>,
) -> Result<Response, ApiError> {
    refuse_escalation(require.grants(), &body.permissions)?;
    let new_role = roles::NewRole {
        key: body.key,
        name: body.name,
        permissions: body.permissions,
    };
    let now = state.clock.now();
    let role = roles::create_custom(&state.db, require.principal().scope(), &new_role, now)
        .await
        .map_err(role_error)?;
    let location = format!("{ROLES_PATH}/{}", role.id);
    let role = Role::from_repo(role)?;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(role),
    )
        .into_response())
}

/// Change a custom role's name or permissions; holders see the change on their next request.
#[utoipa::path(
    patch,
    path = "/api/v1/admin/roles/{role_id}",
    operation_id = "updateRole",
    tag = "roles",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(RoleIdParam),
    request_body = RolePatch,
    responses(
        (status = 200, description = "The changed role", body = Role),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = WriteForbidden),
        (status = 404, response = RoleNotFound),
        (status = 409, response = UpdateConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_role(
    require: Require<RoleManage>,
    State(state): State<AppState>,
    ApiPath(RoleIdParam { role_id }): ApiPath<RoleIdParam>,
    ApiJson(body): ApiJson<RolePatch>,
) -> Result<Json<Role>, ApiError> {
    let scope = require.principal().scope();
    let mut tx = state.db.begin().await.map_err(|e| ApiError::internal(&e))?;
    // Locked until the commit, so the permissions checked below are the ones the update
    // replaces.
    let current = roles::lock_custom(&mut tx, scope, role_id)
        .await
        .map_err(role_error)?;
    if let Some(permissions) = &body.permissions {
        refuse_escalation(
            require.grants(),
            &changed_keys(&current.permissions, permissions),
        )?;
    }
    let patch = roles::RolePatch {
        name: body.name,
        permissions: body.permissions,
    };
    let now = state.clock.now();
    let role = roles::update_custom(&mut *tx, scope, role_id, &patch, now)
        .await
        .map_err(role_error)?;
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    Ok(Json(Role::from_repo(role)?))
}

/// Delete a custom role that has no assignments.
#[utoipa::path(
    delete,
    path = "/api/v1/admin/roles/{role_id}",
    operation_id = "deleteRole",
    tag = "roles",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(RoleIdParam),
    responses(
        (status = 204, description = "The role is deleted"),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = DeleteForbidden),
        (status = 404, response = RoleNotFound),
        (status = 409, response = DeleteConflict),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn delete_role(
    require: Require<RoleManage>,
    State(state): State<AppState>,
    ApiPath(RoleIdParam { role_id }): ApiPath<RoleIdParam>,
) -> Result<StatusCode, ApiError> {
    roles::delete_custom(&state.db, require.principal().scope(), role_id)
        .await
        .map_err(role_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 403 `permission.escalation` when a catalog key in `keys` is not effective for the caller at
/// organization scope (B8, Q4). A key outside the catalog is left to the repository's 422.
fn refuse_escalation(grants: &Grants, keys: &[String]) -> Result<(), ApiError> {
    let unheld = keys.iter().any(|key| {
        perm::ALL.iter().any(|p| p.key() == key) && !grants.holds_key(key, &Target::Organization)
    });
    if !unheld {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::FORBIDDEN,
        code::PERMISSION_ESCALATION,
        "A role can only gain or lose permissions you hold at organization scope.",
    ))
}

/// The keys a role gains or loses when its list `from` becomes `to`, sorted. A key in both is
/// neither: keeping a permission the caller lacks grants no one anything new. `to` is the
/// request's list and is not validated yet, so the sets keep this O(n log n) however long it is.
fn changed_keys(from: &[String], to: &[String]) -> Vec<String> {
    let from: BTreeSet<&String> = from.iter().collect();
    let to: BTreeSet<&String> = to.iter().collect();
    from.symmetric_difference(&to)
        .map(|key| (*key).clone())
        .collect()
}

fn role_not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        code::ROLE_NOT_FOUND,
        "No role with this id exists in this organization.",
    )
}

/// The answer for a roles repository error.
fn role_error(error: roles::Error) -> ApiError {
    use roles::Error as E;
    let invalid = |path: &[&str], code| ApiError::invalid(vec![FieldError::at(path, code)]);
    match error {
        E::NotFound => role_not_found(),
        E::KeyTaken => ApiError::new(
            StatusCode::CONFLICT,
            code::ROLE_KEY_TAKEN,
            "Another role of this organization, or a built-in role, has this key.",
        ),
        E::BuiltinImmutable => ApiError::new(
            StatusCode::CONFLICT,
            code::ROLE_BUILTIN_IMMUTABLE,
            "Built-in roles cannot be changed or deleted. Create a custom role instead.",
        ),
        E::InUse => ApiError::new(
            StatusCode::CONFLICT,
            code::ROLE_IN_USE,
            "The role still has assignments. Remove them first.",
        ),
        E::KeyInvalid => invalid(&["key"], code::ROLE_KEY_INVALID),
        E::NameEmpty => invalid(&["name"], code::NAME_EMPTY),
        E::NameTooLong => invalid(&["name"], code::NAME_TOO_LONG),
        E::ContainsNul { field } => invalid(&[field], code::TEXT_CONTAINS_NUL),
        E::UnknownPermission { index } => invalid(
            &["permissions", &index.to_string()],
            code::ROLE_UNKNOWN_PERMISSION,
        ),
        E::DuplicatePermission { index } => invalid(
            &["permissions", &index.to_string()],
            code::ROLE_DUPLICATE_PERMISSION,
        ),
        other @ (E::BuiltinKeyConflict { .. } | E::Time(_) | E::Database(_)) => {
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
        reason = "unit tests of the escalation rule need grants and have no credential"
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

    fn keys(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|&k| k.to_owned()).collect()
    }

    #[test]
    fn only_catalog_keys_held_at_organization_scope_pass() {
        let g = grants(&["role.manage", "user.view"], &["account.manage"]);
        assert!(refuse_escalation(&g, &keys(&[])).is_ok());
        assert!(refuse_escalation(&g, &keys(&["user.view", "role.manage"])).is_ok());
        // Held only at an account: not enough for an organization-wide definition.
        let refused = refuse_escalation(&g, &keys(&["account.manage"])).unwrap_err();
        assert_eq!(refused.status(), StatusCode::FORBIDDEN);
        assert_eq!(refused.code(), code::PERMISSION_ESCALATION);
        assert!(refuse_escalation(&g, &keys(&["user.view", "user.manage"])).is_err());
        // An unknown key is the repository's 422, not a 403.
        assert!(refuse_escalation(&g, &keys(&["nonexistent.perm"])).is_ok());
    }

    #[test]
    fn changed_keys_are_the_gained_and_the_lost() {
        let from = keys(&["user.view", "user.manage"]);
        assert_eq!(
            changed_keys(&from, &keys(&["user.manage", "role.view"])),
            keys(&["role.view", "user.view"])
        );
        assert!(changed_keys(&from, &keys(&["user.manage", "user.view"])).is_empty());
        let sorted = keys(&["user.manage", "user.view"]);
        assert_eq!(changed_keys(&from, &[]), sorted);
        assert_eq!(changed_keys(&[], &from), sorted);
        // A repeated key in the request changes nothing by being repeated.
        assert_eq!(
            changed_keys(&from, &keys(&["user.view", "user.view", "user.manage"])),
            Vec::<String>::new()
        );
    }

    #[test]
    fn repository_errors_map_to_their_status_code_and_field() {
        let cases = [
            (roles::Error::NotFound, 404, "role.not_found", None),
            (roles::Error::KeyTaken, 409, "role.key_taken", None),
            (
                roles::Error::BuiltinImmutable,
                409,
                "role.builtin_immutable",
                None,
            ),
            (roles::Error::InUse, 409, "role.in_use", None),
            (
                roles::Error::KeyInvalid,
                422,
                "request.invalid",
                Some(("/key", "role.key_invalid")),
            ),
            (
                roles::Error::NameEmpty,
                422,
                "request.invalid",
                Some(("/name", "name.empty")),
            ),
            (
                roles::Error::NameTooLong,
                422,
                "request.invalid",
                Some(("/name", "name.too_long")),
            ),
            (
                roles::Error::ContainsNul { field: "name" },
                422,
                "request.invalid",
                Some(("/name", "text.contains_nul")),
            ),
            (
                roles::Error::UnknownPermission { index: 3 },
                422,
                "request.invalid",
                Some(("/permissions/3", "role.unknown_permission")),
            ),
            (
                roles::Error::DuplicatePermission { index: 2 },
                422,
                "request.invalid",
                Some(("/permissions/2", "role.duplicate_permission")),
            ),
        ];
        for (error, status, code, field) in cases {
            let problem = role_error(error).problem();
            assert_eq!((problem.status, problem.code.as_str()), (status, code));
            let fields: Vec<_> = problem
                .errors
                .iter()
                .map(|e| (e.field.as_str(), e.code.as_str()))
                .collect();
            assert_eq!(fields, field.into_iter().collect::<Vec<_>>(), "{code}");
        }
        let internal = role_error(roles::Error::BuiltinKeyConflict {
            org_id: new_id(),
            key: "org_admin".into(),
        });
        assert_eq!(internal.code(), "internal.error");
    }

    #[test]
    fn scope_types_serialize_in_lowercase() {
        let json = serde_json::to_value([
            ScopeType::from(perm::ScopeType::Organization),
            ScopeType::from(perm::ScopeType::Account),
            ScopeType::from(perm::ScopeType::Course),
        ])
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!(["organization", "account", "course"])
        );
    }
}
