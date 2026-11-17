// SPDX-License-Identifier: AGPL-3.0-or-later
//! Enrollments: `/api/v1/courses/{course_id}/enrollments` and `/api/v1/enrollments/{id}`
//! (docs/specs/m1/02-courses-and-enrollments.md B15 to B20, B24a, B27 and API;
//! docs/specs/m1/05-progress-and-events.md B15, B17 to B19).
//!
//! An enrollment puts a user in a course with a role. An active one grants the role's
//! permissions that are grantable at course scope, at that course only (B16): it is the
//! course-scope grant (Q1). Reading a course's enrollments needs `enrollment.view` at the course;
//! enrolling, concluding, reactivating and removing need `enrollment.manage` there, which the
//! caller may hold at the organization, an account containing the course, or through an
//! enrollment of their own in it.
//!
//! `POST` checks in B17's order: the user, who must exist and be covered by the caller's
//! `user.view` as the users list reaches them (404 `user.not_found` otherwise, owner decision of
//! 2026-11-17), the role (404
//! `role.not_found`), Manager (422 `enrollment.role_unavailable`, until M4), the observed
//! learners present exactly when the role holds `progress.view_observed` (422
//! `enrollment.observed_invalid` at `/observed_user_ids`, spec 05 B17), a built-in role this
//! release does not define (409 `role.unknown_builtin`), the escalation rule (403
//! `permission.escalation`), naming observed learners without `progress.view` at the course
//! (403 `permission.escalation`, spec 05 B19), then the course's own rules in
//! `stepclimb_core::enrollments::create`: an archived course (409 `course.archived`), a
//! template and a role holding `course.participate` (409 `course.template`), the observed
//! learners each a learner of the course (422 `enrollment.observed_invalid` at the entry), and
//! a repeat (409 `enrollment.exists`).
//!
//! The escalation rule (B17, Q12, B24a): the caller must hold at the course every catalog
//! permission of the role that is grantable at organization or account scope, so `enrollment.
//! manage` alone does not let anyone hand out `course.manage` or `organization.manage` through a
//! custom role. A permission grantable only at course scope (`course.participate`) is left out:
//! no assignment can give it to the caller, and it describes the enrolled person's own relation
//! to the course; `progress.view_observed`, the one course-only permission over other people,
//! is guarded instead by spec 05 B19's check on naming observed learners. `PATCH` and `DELETE`
//! run the same check on the enrollment's role (B18), then refuse an enrollment an outside
//! source owns (409 `enrollment.externally_managed`). A `PATCH` that names observed learners,
//! or reactivates an enrollment that observes learners, also needs `progress.view` at the
//! course (403 `permission.escalation`).
//!
//! Every write checks `enrollment.manage` again on the course's path as `courses::lock` reads it
//! in the write's transaction, which also locks the course row (B19), and reads the role
//! `FOR KEY SHARE`, so neither a move of the course's account nor a change of the role's
//! permissions can land between the checks and the write.

use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use stepclimb_core::{
    Source, courses,
    enrollments::{self as repo, ListFilter, NewEnrollment},
    grants::{Denied, Grants, Target},
    id::Uuid,
    perm::{EnrollmentManage, EnrollmentView, Permission, ProgressView, ProgressViewObserved},
    roles, users,
};
use utoipa::{IntoParams, ToSchema};

use crate::{
    AppState,
    audit::ClientAddress,
    error::{ApiError, FieldError},
    extract::{ApiJson, ApiQuery},
    openapi::problem_responses,
    organization::present,
    pagination::{Page, PageParams},
    require::{
        CourseTarget, EnrollmentTarget, Require, check_course,
        code::{COURSE_NOT_FOUND, ENROLLMENT_NOT_FOUND},
        not_found_as,
    },
    role_assignments::{code::USER_NOT_FOUND, refuse_unknown_builtin},
    roles::{code::PERMISSION_ESCALATION, role_not_found},
    timestamp,
    users::user_view_accounts,
};

/// The path of the enrollments of one course, under `/api/v1/courses/{course_id}`.
pub const ENROLLMENTS_SEGMENT: &str = "enrollments";

/// The path an enrollment is at: `{ENROLLMENT_PATH}/{enrollment_id}`.
pub const ENROLLMENT_PATH: &str = "/api/v1/enrollments";

/// Built-in roles that cannot be enrolled yet (B17, Q9): Manager, until M4.
const NOT_ENROLLABLE: [&str; 1] = ["manager"];

/// The codes this module produces besides `course.*` (`crate::courses::code`), the role and
/// user codes, and `enrollment.not_found` (`require::code`).
pub mod code {
    /// The user already holds the role in the course (409).
    pub const ENROLLMENT_EXISTS: &str = "enrollment.exists";
    /// An outside source owns the enrollment (409).
    pub const ENROLLMENT_EXTERNALLY_MANAGED: &str = "enrollment.externally_managed";
    /// In a 422's `errors`: the role cannot be enrolled (Manager).
    pub const ENROLLMENT_ROLE_UNAVAILABLE: &str = "enrollment.role_unavailable";
    /// In a 422's `errors`: the observed learners are missing for a role holding
    /// `progress.view_observed`, present for one without it, not 1 to 100 entries, or (at an
    /// entry) a repeat or a user who is not a learner of the course.
    pub const ENROLLMENT_OBSERVED_INVALID: &str = "enrollment.observed_invalid";
}

problem_responses! {
    EnrollmentListNotFound => "`course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined.",
    EnrollmentCreateNotFound => "`course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `user.not_found`: no user of the organization has this id, or your `user.view` does not cover them (held at the organization, or at an account where they hold a role or an active enrollment). `role.not_found`: no built-in role and no custom role of the organization has this id. `org.not_found`: the request's organization cannot be determined.",
    EnrollmentCreateForbidden => "`permission.denied`: you can see the course but do not hold `enrollment.manage` at a scope containing it. `permission.escalation`: the role has a catalog permission grantable at organization or account scope that you do not hold at the course, or the body names observed learners and you do not hold `progress.view` at the course. `auth.csrf_refused`: the request did not come from this site.",
    EnrollmentCreateConflict => "`enrollment.exists`: the user already holds the role in the course. `role.unknown_builtin`: the role is a built-in role of another Stepclimb release. `course.archived`: an archived course takes no new enrollment. `course.template`: a template takes no enrollment whose role holds `course.participate`.",
    EnrollmentNotFound => "`enrollment.not_found`: no enrollment with this id exists in the organization, or you cannot see its course; a draft course or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined.",
    EnrollmentWriteForbidden => "`permission.denied`: you can see the enrollment's course but do not hold `enrollment.manage` at a scope containing it. `permission.escalation`: the enrollment's role has a catalog permission grantable at organization or account scope that you do not hold at the course, or the change names observed learners or reactivates an enrollment that observes learners and you do not hold `progress.view` at the course. `auth.csrf_refused`: the request did not come from this site.",
    EnrollmentWriteConflict => "`enrollment.externally_managed`: an identity provider, the sync API, SCIM or a connector owns the enrollment. `role.unknown_builtin`: the enrollment's role is a built-in role of another Stepclimb release.",
}

/// An enrollment's status: `active` grants the role at the course, `concluded` grants nothing
/// and is kept as history.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EnrollmentStatus {
    Active,
    Concluded,
}

impl From<repo::Status> for EnrollmentStatus {
    fn from(status: repo::Status) -> Self {
        match status {
            repo::Status::Active => Self::Active,
            repo::Status::Concluded => Self::Concluded,
        }
    }
}

impl From<EnrollmentStatus> for repo::Status {
    fn from(status: EnrollmentStatus) -> Self {
        match status {
            EnrollmentStatus::Active => Self::Active,
            EnrollmentStatus::Concluded => Self::Concluded,
        }
    }
}

/// Who owns an enrollment: an admin of this organization (`local`), or the identity provider,
/// sync API, SCIM or connector that wrote it, in which case it cannot be changed here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum EnrollmentSource {
    Local,
    Idp,
    Sync,
    Scim,
    Connector,
}

impl From<Source> for EnrollmentSource {
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

/// The enrolled user, as much as a roster needs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct EnrolledUser {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub display_name: String,
}

/// A user's enrollment in a course with a role.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Enrollment {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub course_id: Uuid,
    pub user: EnrolledUser,
    #[schema(value_type = String, format = "uuid")]
    pub role_id: Uuid,
    pub status: EnrollmentStatus,
    pub source: EnrollmentSource,
    /// The owning system's id for the enrollment; `null` for a `local` one.
    #[schema(required = true)]
    pub external_id: Option<String>,
    /// The learners this enrollment observes, sorted by id: present when its role holds
    /// `progress.view_observed` (an Observer), absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>, required = false)]
    pub observed_user_ids: Option<Vec<Uuid>>,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub updated_at: String,
}

impl Enrollment {
    fn from_repo(enrollment: repo::Enrollment) -> Result<Self, ApiError> {
        let time = |t| timestamp::rfc3339(t).map_err(|error| ApiError::internal(&error));
        Ok(Self {
            created_at: time(enrollment.created_at)?,
            updated_at: time(enrollment.updated_at)?,
            id: enrollment.id,
            course_id: enrollment.course_id,
            user: EnrolledUser {
                id: enrollment.user_id,
                display_name: enrollment.user_display_name,
            },
            role_id: enrollment.role_id,
            status: enrollment.status.into(),
            source: enrollment.source.into(),
            external_id: enrollment.external_id,
            observed_user_ids: enrollment.observed_user_ids,
        })
    }
}

/// An enrollment to create, `active`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewEnrollmentBody {
    #[schema(value_type = String, format = "uuid")]
    pub user_id: Uuid,
    /// A built-in role other than `manager`, or a custom role of the organization.
    #[schema(value_type = String, format = "uuid")]
    pub role_id: Uuid,
    /// The learners the enrollment observes: required, 1 to 100 distinct learners of the course
    /// (users holding an enrollment there whose role has `course.participate`), when the role
    /// holds `progress.view_observed`, and refused otherwise. Naming them needs `progress.view`
    /// at the course.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Vec<String>)]
    pub observed_user_ids: Option<Vec<Uuid>>,
}

/// A change to an enrollment; an absent member keeps its value.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentPatch {
    /// Reactivating an enrollment that observes learners needs `progress.view` at the course.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = EnrollmentStatus)]
    pub status: Option<EnrollmentStatus>,
    /// The whole new list of learners the enrollment observes, for a role holding
    /// `progress.view_observed`: 1 to 100 distinct learners of the course. Needs `progress.view`
    /// at the course.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Vec<String>)]
    pub observed_user_ids: Option<Vec<Uuid>>,
}

/// The filters of a course's enrollments; each narrows the list.
#[derive(Clone, Copy, Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct EnrollmentListParams {
    #[param(value_type = Option<EnrollmentStatus>)]
    pub status: Option<EnrollmentStatus>,
    #[param(value_type = Option<String>, format = "uuid")]
    pub role_id: Option<Uuid>,
    #[param(value_type = Option<String>, format = "uuid")]
    pub user_id: Option<Uuid>,
}

/// List a course's enrollments, sorted by the user's display name, then the enrollment's id.
#[utoipa::path(
    get,
    path = "/api/v1/courses/{course_id}/enrollments",
    operation_id = "listEnrollments",
    tag = "enrollments",
    params(
        ("course_id" = String, Path, format = "uuid", description = "The course's id."),
        EnrollmentListParams,
        PageParams,
    ),
    responses(
        (status = 200, description = "One page of enrollments", body = Page<Enrollment>),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = EnrollmentListNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_enrollments(
    require: Require<EnrollmentView, CourseTarget>,
    State(state): State<AppState>,
    ApiQuery(filter): ApiQuery<EnrollmentListParams>,
    ApiQuery(params): ApiQuery<PageParams>,
) -> Result<Json<Page<Enrollment>>, ApiError> {
    let page = params.parse::<repo::ListKey>()?;
    let filter = ListFilter {
        status: filter.status.map(Into::into),
        role_id: filter.role_id,
        user_id: filter.user_id,
    };
    let (scope, course_id) = (require.grants().scope(), require.target().id);
    let (after, limit) = (page.after(), page.fetch_limit());
    let rows = repo::list_for_course(&state.db, scope, course_id, filter, after, limit)
        .await
        .map_err(|error| match error {
            // Only the cursor's display name is a caller's string here.
            repo::Error::ContainsNul { .. } => ApiError::invalid_cursor(),
            other => ApiError::internal(&other),
        })?;
    let rows = rows
        .into_iter()
        .map(Enrollment::from_repo)
        .collect::<Result<Vec<_>, _>>()?;
    page.page(rows, |e| (e.user.display_name.clone(), e.id))
        .map(Json)
}

/// Enroll a user in a course with a role, as `active`.
#[utoipa::path(
    post,
    path = "/api/v1/courses/{course_id}/enrollments",
    operation_id = "createEnrollment",
    tag = "enrollments",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("course_id" = String, Path, format = "uuid", description = "The course's id.")),
    request_body = NewEnrollmentBody,
    responses(
        (
            status = 201,
            description = "The enrollment is created",
            body = Enrollment,
            headers(("Location" = String, description = "The enrollment's path"))
        ),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = EnrollmentCreateForbidden),
        (status = 404, response = EnrollmentCreateNotFound),
        (status = 409, response = EnrollmentCreateConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn create_enrollment(
    require: Require<EnrollmentManage, CourseTarget>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<NewEnrollmentBody>,
) -> Result<Response, ApiError> {
    let (grants, course_id) = (require.grants(), require.target().id);
    let scope = grants.scope();
    refuse_unreachable_user(&state, grants, body.user_id).await?;
    let mut tx = begin(&state).await?;
    let target = recheck_manage(&mut tx, grants, course_id, COURSE_NOT_FOUND).await?;
    // Locked until the commit, so the permissions the escalation check reads are the ones the
    // role has when the enrollment is inserted (B19).
    let role = roles::get_for_key_share(&mut tx, scope, body.role_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(role_not_found)?;
    if role.builtin && NOT_ENROLLABLE.contains(&role.key.as_str()) {
        return Err(ApiError::invalid(vec![FieldError::at(
            &["role_id"],
            code::ENROLLMENT_ROLE_UNAVAILABLE,
        )]));
    }
    let observes = role
        .permissions
        .iter()
        .any(|k| k == ProgressViewObserved::KEY);
    if observes != body.observed_user_ids.is_some() {
        return Err(ApiError::invalid(vec![FieldError::at(
            &["observed_user_ids"],
            code::ENROLLMENT_OBSERVED_INVALID,
        )]));
    }
    refuse_unknown_builtin(&role)?;
    refuse_escalation(grants, &role.permissions, &target)?;
    if observes {
        refuse_observing_without_progress_view(grants, &target)?;
    }
    let new = NewEnrollment {
        user_id: body.user_id,
        role_id: body.role_id,
        source: Source::Local,
        external_id: None,
        observed_user_ids: body.observed_user_ids.unwrap_or_default(),
    };
    let actor = client.actor(require.principal());
    let created = repo::create(&mut *tx, scope, course_id, &new, state.clock.now(), actor)
        .await
        .map_err(enrollment_error)?;
    commit(tx).await?;
    let location = format!("{ENROLLMENT_PATH}/{}", created.id);
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(Enrollment::from_repo(created)?),
    )
        .into_response())
}

/// Conclude or reactivate an enrollment, or replace the learners an Observer enrollment
/// observes. A change naming only current values changes nothing.
#[utoipa::path(
    patch,
    path = "/api/v1/enrollments/{enrollment_id}",
    operation_id = "updateEnrollment",
    tag = "enrollments",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("enrollment_id" = String, Path, format = "uuid", description = "The enrollment's id.")),
    request_body = EnrollmentPatch,
    responses(
        (status = 200, description = "The enrollment after the change", body = Enrollment),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = EnrollmentWriteForbidden),
        (status = 404, response = EnrollmentNotFound),
        (status = 409, response = EnrollmentWriteConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_enrollment(
    require: Require<EnrollmentManage, EnrollmentTarget>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<EnrollmentPatch>,
) -> Result<Json<Enrollment>, ApiError> {
    let grants = require.grants();
    let mut tx = begin(&state).await?;
    let (enrollment, course) = writable(&mut tx, grants, require.target()).await?;
    let change = repo::Change {
        status: body.status.map(Into::into),
        observed_user_ids: body.observed_user_ids,
    };
    if widens_observation(&enrollment, &change) {
        refuse_observing_without_progress_view(grants, &course)?;
    }
    let (now, actor) = (state.clock.now(), client.actor(require.principal()));
    let updated = repo::update(&mut *tx, grants.scope(), enrollment.id, &change, now, actor)
        .await
        .map_err(enrollment_error)?;
    commit(tx).await?;
    Ok(Json(Enrollment::from_repo(updated)?))
}

/// Remove an enrollment. When the user keeps no other enrollment in the course whose role holds
/// `course.participate`, this also deletes their progress in the course and the Observer links
/// that name them; an Observer enrollment's own links always go with it.
#[utoipa::path(
    delete,
    path = "/api/v1/enrollments/{enrollment_id}",
    operation_id = "deleteEnrollment",
    tag = "enrollments",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("enrollment_id" = String, Path, format = "uuid", description = "The enrollment's id.")),
    responses(
        (status = 204, description = "The enrollment is removed"),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = EnrollmentWriteForbidden),
        (status = 404, response = EnrollmentNotFound),
        (status = 409, response = EnrollmentWriteConflict),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn delete_enrollment(
    require: Require<EnrollmentManage, EnrollmentTarget>,
    client: ClientAddress,
    State(state): State<AppState>,
) -> Result<StatusCode, ApiError> {
    let grants = require.grants();
    let mut tx = begin(&state).await?;
    let (enrollment, _) = writable(&mut tx, grants, require.target()).await?;
    let (now, actor) = (state.clock.now(), client.actor(require.principal()));
    repo::delete(&mut *tx, grants.scope(), enrollment.id, now, actor)
        .await
        .map_err(enrollment_error)?;
    commit(tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 404 `user.not_found` unless the user exists and the caller's `user.view` covers them: held at
/// the organization, or at an account whose subtree holds one of the user's account-scope role
/// assignments or active enrollments, the reach of the users list (B17, B21, owner decision of
/// 2026-11-17). A user the caller cannot see answers as one that does not exist, so enrolling
/// does not reveal who is in the organization.
async fn refuse_unreachable_user(
    state: &AppState,
    grants: &Grants,
    user_id: Uuid,
) -> Result<(), ApiError> {
    let covered = match user_view_accounts(grants) {
        Some(accounts) if accounts.is_empty() => false,
        within => users::covers(&state.db, grants.scope(), user_id, within)
            .await
            .map_err(|error| ApiError::internal(&error))?,
    };
    if covered {
        Ok(())
    } else {
        Err(user_not_found())
    }
}

/// B18's checks for changing or removing the enrollment `target`, in the write's transaction:
/// `enrollment.manage` again on its course's path as `courses::lock` reads it (404
/// `enrollment.not_found` when the course left the caller's reach), the enrollment itself (404),
/// its role read `FOR KEY SHARE` (409 `role.unknown_builtin` for another release's built-in
/// role), the escalation rule (403) and its source (409 `enrollment.externally_managed`).
/// Returns the enrollment and its course as the evaluator's target.
async fn writable(
    tx: &mut Transaction<'static, Postgres>,
    grants: &Grants,
    target: &EnrollmentTarget,
) -> Result<(repo::Enrollment, Target), ApiError> {
    let scope = grants.scope();
    let course = recheck_manage(tx, grants, target.course.id, ENROLLMENT_NOT_FOUND).await?;
    let hidden = || not_found_as(Denied::CourseNotFound, ENROLLMENT_NOT_FOUND);
    let enrollment = repo::get(&mut **tx, scope, target.id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(hidden)?;
    let role = roles::get_for_key_share(tx, scope, enrollment.role_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(|| ApiError::internal(&RoleMissing))?;
    refuse_unknown_builtin(&role)?;
    refuse_escalation(grants, &role.permissions, &course)?;
    if enrollment.source != Source::Local {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            code::ENROLLMENT_EXTERNALLY_MANAGED,
            "An outside system owns this enrollment. Change it there.",
        ));
    }
    Ok((enrollment, course))
}

/// Whether `change` names observed learners, or reactivates `enrollment` while it observes
/// learners: either gives its holder sight of those learners' progress (spec 05 B19, B20).
fn widens_observation(enrollment: &repo::Enrollment, change: &repo::Change) -> bool {
    let observes = enrollment
        .observed_user_ids
        .as_ref()
        .is_some_and(|users| !users.is_empty());
    let reactivates =
        change.status == Some(repo::Status::Active) && enrollment.status != repo::Status::Active;
    change.observed_user_ids.is_some() || (reactivates && observes)
}

/// 403 `permission.escalation` unless the caller holds `progress.view` at the course (spec 05
/// B19): whoever points an Observer at learners, or restores an Observer's sight of them, must
/// already be able to read those learners' progress. `progress.view_observed` is course-only, so
/// the escalation rule leaves it out (B24a) and this check guards it.
fn refuse_observing_without_progress_view(
    grants: &Grants,
    course: &Target,
) -> Result<(), ApiError> {
    if grants.holds::<ProgressView>(course) {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::FORBIDDEN,
        PERMISSION_ESCALATION,
        "Naming the learners an Observer sees needs `progress.view` at this course.",
    ))
}

/// The extractor's `enrollment.manage` check again ([`check_course`]), on the course as
/// `courses::lock` reads it in the transaction, which locks the course row `FOR NO KEY UPDATE`
/// and its account `FOR SHARE` (B19): 404 `not_found` when the course is gone, out of the
/// caller's reach, or a draft or template without `course.view_all` (B24, Q13); 403 when it is
/// visible without `enrollment.manage`. Returns the course as the evaluator's target.
async fn recheck_manage(
    tx: &mut Transaction<'static, Postgres>,
    grants: &Grants,
    course_id: Uuid,
    not_found: &'static str,
) -> Result<Target, ApiError> {
    let course = courses::lock(tx, grants.scope(), course_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(|| not_found_as(Denied::CourseNotFound, not_found))?;
    let needs_view_all = course.needs_view_all();
    let course = CourseTarget {
        id: course_id,
        account_path: course.account_path,
    };
    check_course::<EnrollmentManage>(grants, &course, needs_view_all, not_found)?;
    Ok(course.target())
}

/// 403 `permission.escalation` when the role has a catalog permission, grantable at
/// organization or account scope, that is not effective for the caller at the course (B17,
/// B18). A permission grantable only at course scope (`course.participate`) is left out (Q12,
/// B24a); a key the catalog does not define grants nothing and is not checked.
fn refuse_escalation(
    grants: &Grants,
    permissions: &[String],
    course: &Target,
) -> Result<(), ApiError> {
    if !grants.lacks_any_of(permissions, course) {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::FORBIDDEN,
        PERMISSION_ESCALATION,
        "You can only enroll with, change or remove a role whose permissions you hold at this course.",
    ))
}

/// An enrollment's role is missing or another organization's: the foreign key and the
/// `enrollments_role_in_org` trigger forbid both, so this is a broken invariant.
#[derive(Debug, thiserror::Error)]
#[error("an enrollment's role is not readable in its organization")]
struct RoleMissing;

async fn begin(state: &AppState) -> Result<Transaction<'static, Postgres>, ApiError> {
    state
        .db
        .begin()
        .await
        .map_err(|error| ApiError::internal(&error))
}

async fn commit(tx: Transaction<'static, Postgres>) -> Result<(), ApiError> {
    tx.commit()
        .await
        .map_err(|error| ApiError::internal(&error))
}

fn user_not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        USER_NOT_FOUND,
        "No user with this id exists in this organization.",
    )
}

/// The answer for an enrollments repository error.
fn enrollment_error(error: repo::Error) -> ApiError {
    use crate::courses::code::{COURSE_ARCHIVED, COURSE_TEMPLATE};
    let observed = |path: &[&str]| {
        ApiError::invalid(vec![FieldError::at(
            path,
            code::ENROLLMENT_OBSERVED_INVALID,
        )])
    };
    use repo::Error as E;
    let conflict = |code, detail| ApiError::new(StatusCode::CONFLICT, code, detail);
    match error {
        E::NotFound => not_found_as(Denied::CourseNotFound, ENROLLMENT_NOT_FOUND),
        E::CourseNotFound => Denied::CourseNotFound.into(),
        E::UserNotFound => user_not_found(),
        E::RoleNotFound => role_not_found(),
        E::Exists => conflict(
            code::ENROLLMENT_EXISTS,
            "The user already holds this role in this course.",
        ),
        E::CourseArchived => conflict(
            COURSE_ARCHIVED,
            "The course is archived and takes no new enrollment.",
        ),
        E::Template => conflict(
            COURSE_TEMPLATE,
            "A template takes no learner. Copy it to an ordinary course first.",
        ),
        E::ObservedUnavailable | E::ObservedInvalid { index: None } => {
            observed(&["observed_user_ids"])
        }
        E::ObservedInvalid { index: Some(i) } => observed(&["observed_user_ids", &i.to_string()]),
        other @ (E::ContainsNul { .. }
        | E::UnknownStoredValue { .. }
        | E::Role(_)
        | E::Time(_)
        | E::Database(_)) => ApiError::internal(&other),
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
    fn grants(at_tower: &[&str], enrolled: &[&str], tower: Uuid, course: Uuid) -> Grants {
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
                grant(
                    at_tower,
                    GrantScope::Account {
                        id: tower,
                        path: vec![tower],
                    },
                ),
                grant(enrolled, GrantScope::Enrolled(course)),
            ],
        )
    }

    fn keys(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|&k| k.to_owned()).collect()
    }

    #[test]
    fn escalation_leaves_out_course_only_permissions_and_nothing_else() {
        let (tower, course) = (new_id(), new_id());
        let target = Target::Course {
            account_path: vec![tower],
            course_id: course,
        };
        let account_admin = grants(&["enrollment.manage", "course.view"], &[], tower, course);
        let ok = |g: &Grants, role: &[&str]| refuse_escalation(g, &keys(role), &target).is_ok();
        // A Student: `course.participate` is left out (T12a).
        assert!(ok(&account_admin, &["course.view", "course.participate"]));
        // ... but it excuses nothing else (T12a).
        let refused = refuse_escalation(
            &account_admin,
            &keys(&["course.participate", "organization.manage"]),
            &target,
        )
        .unwrap_err();
        assert_eq!(refused.status(), StatusCode::FORBIDDEN);
        assert_eq!(refused.code(), "permission.escalation");
        assert!(!ok(&account_admin, &["course.manage"]));
        // An enrollment's own grants count at its course (T12).
        let instructor = grants(
            &[],
            &["course.view", "course.view_all", "enrollment.manage"],
            tower,
            course,
        );
        assert!(ok(&instructor, &["course.view", "course.view_all"]));
        assert!(!ok(&instructor, &["course.view", "course.manage"]));
        assert!(ok(&instructor, &["gone.key"]));
    }

    #[test]
    fn repository_errors_map_to_their_status_and_code() {
        let cases = [
            (repo::Error::NotFound, 404, "enrollment.not_found"),
            (repo::Error::CourseNotFound, 404, "course.not_found"),
            (repo::Error::UserNotFound, 404, "user.not_found"),
            (repo::Error::RoleNotFound, 404, "role.not_found"),
            (repo::Error::Exists, 409, "enrollment.exists"),
            (repo::Error::CourseArchived, 409, "course.archived"),
            (repo::Error::Template, 409, "course.template"),
        ];
        for (error, status, code) in cases {
            let problem = enrollment_error(error).problem();
            assert_eq!((problem.status, problem.code.as_str()), (status, code));
        }
    }

    #[test]
    fn statuses_and_sources_serialize_as_their_stored_spelling() {
        for status in [repo::Status::Active, repo::Status::Concluded] {
            let json = serde_json::to_value(EnrollmentStatus::from(status)).unwrap();
            assert_eq!(json, status.as_str());
        }
        for source in Source::ALL {
            let json = serde_json::to_value(EnrollmentSource::from(source)).unwrap();
            assert_eq!(json, source.as_str());
        }
    }
}
