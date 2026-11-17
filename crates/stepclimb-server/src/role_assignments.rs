// SPDX-License-Identifier: AGPL-3.0-or-later
//! The role assignment routes under `/api/v1/admin/role-assignments`
//! (docs/specs/m0/09-permissions-and-roles.md B11 to B14 and API).
//!
//! An assignment grants one role to one user at one scope. Listing needs `role.view` somewhere
//! and shows only assignments at scopes where the caller holds it. Creating and removing need
//! `role.assign` at the assignment's scope and the escalation rule (Q4): the caller must hold,
//! at that scope, every catalog permission of the role, so an account admin cannot hand out or
//! take away Organization admin at its account.
//!
//! `POST` checks in B12's order: the user (404 `user.not_found`), the role (404
//! `role.not_found`; 409 `role.unknown_builtin` for a built-in role this release does not
//! define), Manager (422, B13), the scope (404, or 422 for a course), the caller's
//! `role.assign` there (404 when the scope is hidden from the caller, else 403), the escalation
//! rule (403), then the repository's refusals: Student and Observer at the organization or an
//! account (422 `role_assignment.course_role`: course-scope roles, granted only by an
//! enrollment, M1 spec 02 B22) and a repeat (409). Both writes take the account tree lock before
//! they read the scope's path, so an account move or delete cannot carry the scope out of the
//! caller's reach between the check and the write. The role is read `FOR KEY SHARE` in the
//! transaction that inserts, so a concurrent change of the role waits until the insert commits.
//! `DELETE` reads the assignment (404), applies the same permission, unknown-role and
//! escalation checks at its scope in the transaction that deletes (a hidden scope answers 404
//! `role_assignment.not_found`), refuses an assignment an outside source owns (409), and leaves
//! the last-admin rule (409) to `stepclimb_core::role_assignments::delete`, which serializes
//! removals per organization.
//!
//! A course-scope assignment cannot be created: an active enrollment is the course-scope grant
//! (M1 spec 02 Q1). One that exists anyway grants nothing and is treated as inside the
//! organization only: an organization-scope grant reaches it, an account grant does not.

use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, Postgres, Transaction};
use stepclimb_core::{
    Source, accounts,
    grants::{self, Denied, Grants, RequireError, ScopeId},
    id::Uuid,
    perm::{Permission, RoleAssign, RoleView},
    role_assignments::{self as repo, ListFilter, NewAssignment, Scope},
    roles, users,
};
use utoipa::{IntoParams, ToSchema};

use crate::{
    AppState,
    audit::ClientAddress,
    error::{ApiError, FieldError},
    extract::{ApiJson, ApiPath, ApiQuery},
    me::GrantScope,
    openapi::problem_responses,
    pagination::{Page, PageParams},
    require::{AnyScope, Require},
    roles::{ScopeType, role_not_found},
    timestamp,
};

/// The path of the assignments collection; an assignment is at
/// `{ROLE_ASSIGNMENTS_PATH}/{assignment_id}`.
pub const ROLE_ASSIGNMENTS_PATH: &str = "/api/v1/admin/role-assignments";

/// The built-in role whose scope (reporting lines) does not exist yet (B13).
const NOT_ASSIGNABLE_YET: [&str; 1] = ["manager"];

/// The codes this module produces.
pub mod code {
    /// No user with this id in the organization (404).
    pub const USER_NOT_FOUND: &str = "user.not_found";
    /// No assignment with this id in the organization, or one at a scope the caller cannot see
    /// (404).
    pub const ROLE_ASSIGNMENT_NOT_FOUND: &str = "role_assignment.not_found";
    /// The user already holds the role at the scope (409).
    pub const ROLE_ASSIGNMENT_EXISTS: &str = "role_assignment.exists";
    /// An outside source owns the assignment (409).
    pub const ROLE_ASSIGNMENT_EXTERNALLY_MANAGED: &str = "role_assignment.externally_managed";
    /// The assignment is the last organization-scope Organization admin of an active user
    /// (409).
    pub const ROLE_ASSIGNMENT_LAST_ORG_ADMIN: &str = "role_assignment.last_org_admin";
    /// The role is a built-in role this server's release does not define (409).
    pub const ROLE_UNKNOWN_BUILTIN: &str = "role.unknown_builtin";
    /// In a 422's `errors`: assignments of this role, or at this scope type, are not available
    /// yet.
    pub const ROLE_ASSIGNMENT_SCOPE_UNAVAILABLE: &str = "role_assignment.scope_unavailable";
    /// In a 422's `errors`: the role is a course-scope role (Student, Observer), which only an
    /// enrollment grants.
    pub const ROLE_ASSIGNMENT_COURSE_ROLE: &str = "role_assignment.course_role";
}

problem_responses! {
    AssignmentListNotFound => "`account.not_found`: the scope filter names no account of the organization, or one you cannot see. `org.not_found`: the scope filter names another organization, or the request's organization cannot be determined.",
    AssignmentCreateForbidden => "`permission.denied`: the caller does not hold `role.assign` at the scope. `permission.escalation`: the role has a catalog permission the caller does not hold at the scope, counting permissions that take no effect there. `auth.csrf_refused`: the request did not come from this site.",
    AssignmentCreateNotFound => "`user.not_found`: no user of the organization has this id. `role.not_found`: no built-in role and no custom role of the organization has this id. `account.not_found`: the scope names no account of the organization, or one you cannot see. `org.not_found`: the scope names another organization, or the request's organization cannot be determined.",
    AssignmentCreateConflict => "`role_assignment.exists`: the user already holds the role at the scope. `role.unknown_builtin`: the role is a built-in role of another Stepclimb release.",
    AssignmentDeleteForbidden => "`permission.denied`: the caller does not hold `role.assign` at the assignment's scope. `permission.escalation`: the role has a catalog permission the caller does not hold at the scope, counting permissions that take no effect there. `auth.csrf_refused`: the request did not come from this site.",
    AssignmentDeleteNotFound => "`role_assignment.not_found`: no assignment of the organization has this id, or its scope is hidden from you. `org.not_found`: the request's organization cannot be determined.",
    AssignmentDeleteConflict => "`role.unknown_builtin`: the assignment's role is a built-in role of another Stepclimb release. `role_assignment.externally_managed`: an identity provider, the sync API, SCIM or a connector owns the assignment. `role_assignment.last_org_admin`: it is the organization's last organization-scope Organization admin assignment held by an active user.",
}

/// A role granted to a user at a scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct RoleAssignment {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub user_id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub role_id: Uuid,
    pub scope: GrantScope,
    pub source: AssignmentSource,
    /// The owning system's id for the assignment; `null` for a `local` one.
    #[schema(required = true)]
    pub external_id: Option<String>,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
}

/// Who owns an assignment: an admin of this organization (`local`), or the identity provider,
/// sync API, SCIM or connector that wrote it, in which case it cannot be removed here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum AssignmentSource {
    Local,
    Idp,
    Sync,
    Scim,
    Connector,
}

impl From<Source> for AssignmentSource {
    fn from(source: Source) -> Self {
        match source {
            Source::Local => Self::Local,
            Source::Idp => Self::Idp,
            Source::Sync => Self::Sync,
            Source::Scim => Self::Scim,
            Source::Connector => Self::Connector,
        }
    }
}

impl RoleAssignment {
    fn from_repo(org_id: Uuid, assignment: repo::RoleAssignment) -> Result<Self, ApiError> {
        let (scope_type, id) = match assignment.scope {
            Scope::Organization => (ScopeType::Organization, org_id),
            Scope::Account(id) => (ScopeType::Account, id),
            Scope::Course(id) => (ScopeType::Course, id),
        };
        Ok(Self {
            id: assignment.id,
            user_id: assignment.user_id,
            role_id: assignment.role_id,
            scope: GrantScope { scope_type, id },
            source: assignment.source.into(),
            external_id: assignment.external_id,
            created_at: timestamp::rfc3339(assignment.created_at)
                .map_err(|error| ApiError::internal(&error))?,
        })
    }
}

/// An assignment to create.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewRoleAssignment {
    #[schema(value_type = String, format = "uuid")]
    pub user_id: Uuid,
    /// A built-in role other than `manager`, `student` and `observer`, or a custom role of the
    /// organization. Student and Observer are granted by enrolling the user in a course.
    #[schema(value_type = String, format = "uuid")]
    pub role_id: Uuid,
    /// The organization (its own id) or an account of it. `course` is not available: the
    /// course-scope grant is an enrollment.
    pub scope: ScopeRef,
}

/// The scope a new assignment takes effect at.
#[derive(Clone, Copy, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopeRef {
    #[serde(rename = "type")]
    pub scope_type: ScopeType,
    /// The organization's own id for `organization`.
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
}

/// The filters of the list; each narrows it.
#[derive(Clone, Copy, Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AssignmentFilter {
    /// Only this user's assignments.
    #[param(value_type = Option<String>, format = "uuid")]
    pub user_id: Option<Uuid>,
    /// Only assignments at this exact scope; needs `scope_id`.
    pub scope_type: Option<ScopeType>,
    /// The scope's id (the organization's own id for `organization`); needs `scope_type`.
    #[param(value_type = Option<String>, format = "uuid")]
    pub scope_id: Option<Uuid>,
}

/// The path parameter of a single assignment's route.
#[derive(Clone, Copy, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Path)]
pub struct AssignmentIdParam {
    /// The assignment's id.
    #[param(value_type = String, format = "uuid")]
    pub assignment_id: Uuid,
}

/// List the role assignments at scopes where the caller holds `role.view`, by id.
#[utoipa::path(
    get,
    path = "/api/v1/admin/role-assignments",
    operation_id = "listRoleAssignments",
    tag = "roles",
    params(AssignmentFilter, PageParams),
    responses(
        (status = 200, description = "One page of assignments", body = Page<RoleAssignment>),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = AssignmentListNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_role_assignments(
    require: Require<RoleView, AnyScope>,
    State(state): State<AppState>,
    ApiQuery(params): ApiQuery<PageParams>,
    ApiQuery(filter): ApiQuery<AssignmentFilter>,
) -> Result<Json<Page<RoleAssignment>>, ApiError> {
    let page = params.parse::<Uuid>()?;
    let grants = require.grants();
    let org_id = grants.scope().org_id();
    let scope = match (filter.scope_type, filter.scope_id) {
        (None, None) => None,
        (Some(scope_type), Some(id)) => {
            let scope = requested_scope(org_id, scope_type, id)?;
            let mut conn = state
                .db
                .acquire()
                .await
                .map_err(|e| ApiError::internal(&e))?;
            permission_at::<RoleView>(&mut conn, grants, scope).await?;
            Some(scope)
        }
        _ => {
            return Err(ApiError::malformed(
                "`scope_type` and `scope_id` are given together or not at all.",
            ));
        }
    };
    let within = visible_accounts::<RoleView>(grants);
    let filter = ListFilter {
        user_id: filter.user_id,
        scope,
        within: within.as_deref(),
    };
    let rows = repo::list(
        &state.db,
        grants.scope(),
        filter,
        page.after().copied(),
        page.fetch_limit(),
    )
    .await
    .map_err(|error| ApiError::internal(&error))?;
    let rows = rows
        .into_iter()
        .map(|row| RoleAssignment::from_repo(org_id, row))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(page.page(rows, |assignment| assignment.id)?))
}

/// Grant a role to a user at a scope.
#[utoipa::path(
    post,
    path = "/api/v1/admin/role-assignments",
    operation_id = "createRoleAssignment",
    tag = "roles",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = NewRoleAssignment,
    responses(
        (
            status = 201,
            description = "The assignment is created",
            body = RoleAssignment,
            headers(("Location" = String, description = "The assignment's path"))
        ),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = AssignmentCreateForbidden),
        (status = 404, response = AssignmentCreateNotFound),
        (status = 409, response = AssignmentCreateConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn create_role_assignment(
    require: Require<RoleAssign, AnyScope>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<NewRoleAssignment>,
) -> Result<Response, ApiError> {
    let grants = require.grants();
    let scope = grants.scope();
    users::get(&state.db, scope, body.user_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(user_not_found)?;
    let mut tx = begin_with_tree_locked(&state, grants).await?;
    // Locked until the commit, so the permissions the escalation check reads are the ones the
    // role has when the assignment is inserted.
    let role = roles::get_for_key_share(&mut tx, scope, body.role_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(role_not_found)?;
    refuse_unknown_builtin(&role)?;
    if role.builtin && NOT_ASSIGNABLE_YET.contains(&role.key.as_str()) {
        return Err(scope_unavailable(&["role_id"]));
    }
    let at = requested_scope(scope.org_id(), body.scope.scope_type, body.scope.id)?;
    if let Scope::Course(_) = at {
        return Err(scope_unavailable(&["scope", "type"]));
    }
    let target = permission_at::<RoleAssign>(&mut tx, grants, at).await?;
    refuse_escalation(grants, &role.permissions, &target)?;
    let new = NewAssignment {
        user_id: body.user_id,
        role_id: body.role_id,
        scope: at,
        source: Source::Local,
        external_id: None,
    };
    let actor = client.actor(require.principal());
    let created = repo::create(&mut *tx, scope, &new, state.clock.now(), actor)
        .await
        .map_err(assignment_error)?;
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    let location = format!("{ROLE_ASSIGNMENTS_PATH}/{}", created.id);
    let created = RoleAssignment::from_repo(scope.org_id(), created)?;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(created),
    )
        .into_response())
}

/// Remove a role assignment.
#[utoipa::path(
    delete,
    path = "/api/v1/admin/role-assignments/{assignment_id}",
    operation_id = "deleteRoleAssignment",
    tag = "roles",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(AssignmentIdParam),
    responses(
        (status = 204, description = "The assignment is removed"),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = AssignmentDeleteForbidden),
        (status = 404, response = AssignmentDeleteNotFound),
        (status = 409, response = AssignmentDeleteConflict),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn delete_role_assignment(
    require: Require<RoleAssign, AnyScope>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiPath(AssignmentIdParam { assignment_id }): ApiPath<AssignmentIdParam>,
) -> Result<StatusCode, ApiError> {
    let grants = require.grants();
    let scope = grants.scope();
    let mut tx = begin_with_tree_locked(&state, grants).await?;
    let assignment = repo::get(&mut *tx, scope, assignment_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(assignment_not_found)?;
    let target = permission_at::<RoleAssign>(&mut tx, grants, assignment.scope)
        .await
        .map_err(|error| match error.code() {
            crate::require::code::ACCOUNT_NOT_FOUND => assignment_not_found(),
            _ => error,
        })?;
    let role = roles::get_for_key_share(&mut tx, scope, assignment.role_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(|| ApiError::internal(&RoleMissing))?;
    refuse_unknown_builtin(&role)?;
    refuse_escalation(grants, &role.permissions, &target)?;
    if assignment.source != Source::Local {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            code::ROLE_ASSIGNMENT_EXTERNALLY_MANAGED,
            "An outside system owns this assignment. Change it there.",
        ));
    }
    let actor = client.actor(require.principal());
    repo::delete(&mut *tx, scope, assignment_id, state.clock.now(), actor)
        .await
        .map_err(assignment_error)?;
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

/// A transaction holding the organization's account tree lock (`accounts::lock_tree`, spec 02
/// B22) until it ends, for a write whose permission check reads an account's path: no account
/// move or delete runs between the check and the write, so an account cannot leave the caller's
/// reach in between.
async fn begin_with_tree_locked(
    state: &AppState,
    grants: &Grants,
) -> Result<Transaction<'static, Postgres>, ApiError> {
    let mut tx = state.db.begin().await.map_err(|e| ApiError::internal(&e))?;
    accounts::lock_tree(&mut tx, grants.scope())
        .await
        .map_err(|error| ApiError::internal(&error))?;
    Ok(tx)
}

/// The scope a request names. An organization scope must name the request's own organization
/// (404 `org.not_found` otherwise).
fn requested_scope(org_id: Uuid, scope_type: ScopeType, id: Uuid) -> Result<Scope, ApiError> {
    match scope_type {
        ScopeType::Organization if id == org_id => Ok(Scope::Organization),
        ScopeType::Organization => Err(Denied::OrganizationNotFound.into()),
        ScopeType::Account => Ok(Scope::Account(id)),
        ScopeType::Course => Ok(Scope::Course(id)),
    }
}

/// B17 steps 3 to 5 for `P` at `scope`: the target's place in the organization, 404 when the
/// account does not exist or is hidden from the caller, 403 when it is visible but `P` is not
/// effective there. A course counts as inside the organization only (see the module docs).
async fn permission_at<P: Permission>(
    conn: &mut PgConnection,
    grants: &Grants,
    scope: Scope,
) -> Result<grants::Target, ApiError> {
    let at = match scope {
        Scope::Organization => ScopeId::Organization(grants.scope().org_id()),
        Scope::Account(id) => ScopeId::Account(id),
        Scope::Course(_) => {
            grants.check::<P>(&grants::Target::Organization)?;
            return Ok(grants::Target::Organization);
        }
    };
    grants
        .require_at::<P>(conn, at)
        .await
        .map_err(|error: RequireError| error.into())
}

/// The accounts whose subtrees the list is limited to: `None` when `P` is effective at the
/// organization, which contains every assignment.
fn visible_accounts<P: Permission>(grants: &Grants) -> Option<Vec<Uuid>> {
    let mut accounts = Vec::new();
    for scope in grants.scopes_with::<P>() {
        match scope {
            ScopeId::Organization(_) => return None,
            ScopeId::Account(id) => accounts.push(id),
        }
    }
    Some(accounts)
}

/// 403 `permission.escalation` when the role has a catalog permission that is not effective for
/// the caller at `target`, the assignment's scope (B12 step 5, B14, Q4). Every catalog key of the
/// role grantable at organization or account scope counts, also one not grantable at the scope's
/// type, so an account admin cannot hand out or take away Organization admin at its own account.
/// A key the catalog does not define grants nothing anywhere and is not checked, nor is one
/// grantable only at course scope, which no assignment makes effective (M1 spec 02 B24a).
fn refuse_escalation(
    grants: &Grants,
    permissions: &[String],
    target: &grants::Target,
) -> Result<(), ApiError> {
    if !grants.lacks_any_of(permissions, target) {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::FORBIDDEN,
        crate::roles::code::PERMISSION_ESCALATION,
        "You can only grant or remove a role whose permissions you hold at its scope.",
    ))
}

/// 409 `role.unknown_builtin` for a built-in role this server does not define, such as a newer
/// release's during a rolling deploy: it grants nothing here, so the escalation check could not
/// see what it grants on the release that defines it, and the request fails closed.
pub(crate) fn refuse_unknown_builtin(role: &roles::Role) -> Result<(), ApiError> {
    if !role.builtin || roles::builtin(&role.key).is_some() {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::CONFLICT,
        code::ROLE_UNKNOWN_BUILTIN,
        "This built-in role comes from another Stepclimb release. Try again once every server runs it.",
    ))
}

/// An assignment's role is missing or another organization's: the foreign key and the
/// `role_assignments_role_in_org` trigger forbid both, so this is a broken invariant.
#[derive(Debug, thiserror::Error)]
#[error("a role assignment's role is not readable in its organization")]
struct RoleMissing;

fn user_not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        code::USER_NOT_FOUND,
        "No user with this id exists in this organization.",
    )
}

fn assignment_not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        code::ROLE_ASSIGNMENT_NOT_FOUND,
        "No role assignment with this id exists, or you cannot see it.",
    )
}

/// 422 `request.invalid` with `role_assignment.scope_unavailable` at `path`.
fn scope_unavailable(path: &[&str]) -> ApiError {
    ApiError::invalid(vec![FieldError::at(
        path,
        code::ROLE_ASSIGNMENT_SCOPE_UNAVAILABLE,
    )])
}

/// The answer for a role assignments repository error.
fn assignment_error(error: repo::Error) -> ApiError {
    use repo::Error as E;
    match error {
        E::NotFound => assignment_not_found(),
        E::UserNotFound => user_not_found(),
        E::RoleNotFound => role_not_found(),
        E::AccountNotFound => Denied::AccountNotFound.into(),
        E::ScopeUnavailable => scope_unavailable(&["scope", "type"]),
        E::CourseRole => ApiError::invalid(vec![FieldError::at(
            &["role_id"],
            code::ROLE_ASSIGNMENT_COURSE_ROLE,
        )]),
        E::Exists => ApiError::new(
            StatusCode::CONFLICT,
            code::ROLE_ASSIGNMENT_EXISTS,
            "The user already holds this role at this scope.",
        ),
        E::LastOrgAdmin => ApiError::new(
            StatusCode::CONFLICT,
            code::ROLE_ASSIGNMENT_LAST_ORG_ADMIN,
            "This is the last Organization admin. Make someone else Organization admin first.",
        ),
        other @ (E::ContainsNul { .. }
        | E::UnknownStoredValue { .. }
        | E::MissingAccount { .. }
        | E::MissingCourse { .. }
        | E::IsolationLevel { .. }
        | E::Time(_)
        | E::Database(_)) => ApiError::internal(&other),
    }
}

#[cfg(test)]
mod tests {
    use stepclimb_core::{
        OrgScope,
        id::new_id,
        role_assignments::{GrantScope as RepoScope, UserGrant},
    };

    use super::*;

    #[expect(
        clippy::disallowed_methods,
        reason = "unit tests of the escalation rule need grants and have no credential"
    )]
    fn grants(org: &[&str], account: &[&str], account_id: Uuid) -> Grants {
        let grant = |keys: &[&str], scope| UserGrant {
            assignment_id: new_id(),
            role_id: new_id(),
            role_key: "custom".into(),
            permissions: keys.iter().map(|&k| k.to_owned()).collect(),
            scope,
        };
        Grants::from_assignments(
            OrgScope::instance(new_id()),
            [
                grant(org, RepoScope::Organization),
                grant(
                    account,
                    RepoScope::Account {
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
    fn escalation_checks_every_catalog_permission_at_the_target() {
        let tower = new_id();
        let at_tower = grants::Target::Account(vec![tower]);
        let at_org = grants::Target::Organization;
        let g = grants(&["user.view"], &["account.view", "role.assign"], tower);
        let ok = |keys_: &[&str], target| refuse_escalation(&g, &keys(keys_), target).is_ok();
        assert!(ok(&["account.view", "user.view"], &at_tower));
        assert!(ok(&[], &at_tower));
        // An organization-only key counts at an account too.
        assert!(!ok(&["account.view", "organization.manage"], &at_tower));
        let refused = refuse_escalation(&g, &keys(&["account.manage"]), &at_tower).unwrap_err();
        assert_eq!(refused.status(), StatusCode::FORBIDDEN);
        assert_eq!(refused.code(), "permission.escalation");
        // At the organization, the account grant does not count.
        assert!(ok(&["user.view"], &at_org));
        assert!(!ok(&["account.view"], &at_org));
        // A key outside the catalog grants nothing and is not checked.
        assert!(ok(&["gone.key"], &at_tower));
        // Nor is a key grantable only at course scope (M1 spec 02 B24a), but it excuses
        // nothing else.
        assert!(ok(&["course.participate", "account.view"], &at_tower));
        assert!(!ok(&["course.participate", "account.manage"], &at_tower));
    }

    #[test]
    fn an_organization_scope_must_name_the_own_organization() {
        let org = new_id();
        let id = new_id();
        assert_eq!(
            requested_scope(org, ScopeType::Organization, org).unwrap(),
            Scope::Organization
        );
        assert_eq!(
            requested_scope(org, ScopeType::Organization, id)
                .unwrap_err()
                .code(),
            "org.not_found"
        );
        assert_eq!(
            requested_scope(org, ScopeType::Account, id).unwrap(),
            Scope::Account(id)
        );
        assert_eq!(
            requested_scope(org, ScopeType::Course, id).unwrap(),
            Scope::Course(id)
        );
    }

    #[test]
    fn visible_accounts_are_none_with_an_organization_grant() {
        let tower = new_id();
        let at_org = grants(&["role.view"], &["role.view"], tower);
        assert_eq!(visible_accounts::<RoleView>(&at_org), None);
        let at_tower = grants(&[], &["role.view"], tower);
        assert_eq!(visible_accounts::<RoleView>(&at_tower), Some(vec![tower]));
    }

    #[test]
    fn repository_errors_map_to_their_status_and_code() {
        let cases = [
            (repo::Error::NotFound, 404, "role_assignment.not_found"),
            (repo::Error::UserNotFound, 404, "user.not_found"),
            (repo::Error::RoleNotFound, 404, "role.not_found"),
            (repo::Error::AccountNotFound, 404, "account.not_found"),
            (repo::Error::ScopeUnavailable, 422, "request.invalid"),
            (repo::Error::CourseRole, 422, "request.invalid"),
            (repo::Error::Exists, 409, "role_assignment.exists"),
            (
                repo::Error::LastOrgAdmin,
                409,
                "role_assignment.last_org_admin",
            ),
            (
                repo::Error::IsolationLevel {
                    found: "serializable".into(),
                },
                500,
                "internal.error",
            ),
        ];
        for (error, status, code) in cases {
            let problem = assignment_error(error).problem();
            assert_eq!((problem.status, problem.code.as_str()), (status, code));
        }
    }

    /// The API's spellings are the stored ones.
    #[test]
    fn sources_serialize_as_their_stored_spelling() {
        for source in Source::ALL {
            let json = serde_json::to_value(AssignmentSource::from(source)).unwrap();
            assert_eq!(json, source.as_str());
        }
    }
}
