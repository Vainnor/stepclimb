// SPDX-License-Identifier: AGPL-3.0-or-later
//! Courses under `/api/v1/courses` (docs/specs/m1/02-courses-and-enrollments.md B1 to B14, B24,
//! B27 and API).
//!
//! A course sits in one account, and course permissions held at an account cover every course
//! in its subtree (B23, B24). Reading needs `course.view` at the course. A draft or a template
//! is hidden from a caller without `course.view_all` there on every route: it answers 404 as a
//! course that does not exist, before the route's own permission is checked (B10, B24, Q13;
//! `require::check_course`). Creating needs `course.create` at the account named in the body; changing details,
//! status or account needs `course.manage` at the course, and a move also `course.create` at
//! the destination (B9). A destination where the caller lacks `course.create` answers 404, as
//! an account the caller cannot see does, and a move may not give the caller a permission
//! grantable at course scope over the course that it lacks there now (403
//! `permission.escalation`), as an account move may not.
//!
//! Copying (B12) needs `course.view_all` at the source, since a copy carries everything, and
//! `course.create` at the destination, with the same 404 for a destination without it. A copy
//! is refused with 403 `permission.escalation` when the destination would give the caller a
//! permission grantable at course scope over the course that it does not hold at the source:
//! the check a move makes, applied to the source and the copy's place (coordinator decision,
//! spec 02 I3; course-scope permissions only, owner decision of 2026-11-16). The copy is a
//! draft and carries no enrollment (Q1, Q12: no learner and no `course.participate` comes with
//! it).
//!
//! The `Require` extractor checks the course before the handler runs. Every write checks again
//! on the course's path as its own transaction reads it: create, copy and move under the
//! organization's account tree lock (`stepclimb_core::accounts::lock_tree`, B2, B9), which every
//! account move takes; update and status change on `courses::lock`, which reads the course's
//! account `FOR SHARE` and so waits for a move in progress over it. A concurrent account move
//! cannot carry the course, its account or the destination out of the caller's reach unseen.

use std::time::SystemTime;

use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::{Postgres, Transaction};
use stepclimb_core::{
    OrgScope, accounts,
    courses::{self, DateField},
    grants::{Denied, Grants, ScopeId, Target},
    id::Uuid,
    perm::{self, CourseCreate, CourseManage, CourseView, CourseViewAll, Permission, ScopeType},
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
    require::{AnyScope, CourseTarget, Require, check_course, code::COURSE_NOT_FOUND},
    roles::code::{NAME_EMPTY, NAME_TOO_LONG, PERMISSION_ESCALATION, TEXT_CONTAINS_NUL},
    timestamp,
    users::code::TEXT_TOO_LONG,
};

/// The path of the courses collection; a course is at `{COURSES_PATH}/{course_id}`.
pub const COURSES_PATH: &str = "/api/v1/courses";

/// The codes this module produces besides `course.not_found` and `account.not_found`
/// (`require::code`).
pub mod code {
    /// Another course of the organization has the code, ignoring ASCII case (409).
    pub const COURSE_CODE_TAKEN: &str = "course.code_taken";
    /// B4 does not allow the status change (409).
    pub const COURSE_STATUS_INVALID: &str = "course.status_invalid";
    /// A template cannot be published (409).
    pub const COURSE_TEMPLATE: &str = "course.template";
    /// `end_at` is before `start_at` (a 422 field code).
    pub const COURSE_DATES_INVERTED: &str = "course.dates_inverted";
    /// A date before 1970 or from 2200 on (a 422 field code).
    pub const COURSE_DATE_OUT_OF_RANGE: &str = "course.date_out_of_range";
}

problem_responses! {
    CourseNotFound => "`course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined.",
    CourseListNotFound => "`account.not_found`: no account with `account_id` exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined.",
    CourseCreateNotFound => "`account.not_found`: no account with `account_id` exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined.",
    CourseMoveNotFound => "`course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `account.not_found`: the destination `account_id` does not exist or you do not hold `course.create` at a scope containing it. `org.not_found`: the request's organization cannot be determined.",
    CourseCreateForbidden => "`permission.denied`: you hold `course.create` nowhere, or you can see the account without holding it there. `auth.csrf_refused`: the request did not come from this site.",
    CourseManageForbidden => "`permission.denied`: you can see the course but do not hold `course.manage` at a scope containing it. `auth.csrf_refused`: the request did not come from this site.",
    CourseMoveForbidden => "`permission.denied`: you can see the course but do not hold `course.manage` at a scope containing it. `permission.escalation`: the destination would give you, over the course, a course permission (one grantable at course scope) you do not hold over it now. `auth.csrf_refused`: the request did not come from this site.",
    CourseCopyNotFound => "`course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `account.not_found`: the destination `account_id` does not exist or you do not hold `course.create` at a scope containing it. `org.not_found`: the request's organization cannot be determined.",
    CourseCopyForbidden => "`permission.denied`: you can see the course but do not hold `course.view_all` at a scope containing it. `permission.escalation`: the destination would give you, over the copy, a course permission (one grantable at course scope) you do not hold over the source. `auth.csrf_refused`: the request did not come from this site.",
    CourseCodeConflict => "`course.code_taken`: another course of the organization has this code, ignoring ASCII case.",
    CourseStatusConflict => "`course.status_invalid`: the course cannot change from its status to this one; the allowed changes are draft to published, published to archived, archived to published and published to draft. `course.template`: a template cannot be published.",
}

/// A course's status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CourseStatus {
    Draft,
    Published,
    Archived,
}

impl From<courses::Status> for CourseStatus {
    fn from(status: courses::Status) -> Self {
        match status {
            courses::Status::Draft => Self::Draft,
            courses::Status::Published => Self::Published,
            courses::Status::Archived => Self::Archived,
        }
    }
}

impl From<CourseStatus> for courses::Status {
    fn from(status: CourseStatus) -> Self {
        match status {
            CourseStatus::Draft => Self::Draft,
            CourseStatus::Published => Self::Published,
            CourseStatus::Archived => Self::Archived,
        }
    }
}

/// What a learner sees first when opening the course.
//
// A struct with a `kind` rather than an internally tagged enum, which serde would buffer
// (`tests/body_shapes.rs`); spec 07 adds the `page` kind and its `page_id`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CourseHome {
    pub kind: HomeKind,
}

/// The kind of a course's home: `modules`, the module list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HomeKind {
    Modules,
}

impl From<courses::Home> for CourseHome {
    fn from(home: courses::Home) -> Self {
        let kind = match home {
            courses::Home::Modules => HomeKind::Modules,
        };
        Self { kind }
    }
}

impl From<CourseHome> for courses::Home {
    fn from(home: CourseHome) -> Self {
        match home.kind {
            HomeKind::Modules => Self::Modules,
        }
    }
}

/// A course's settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct CourseSettings {
    pub home: CourseHome,
}

/// A course of the caller's organization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Course {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub account_id: Uuid,
    pub title: String,
    pub code: String,
    pub status: CourseStatus,
    /// A template is never published and takes no learners.
    pub template: bool,
    /// RFC 3339, UTC. Shown to learners and staff; it opens and closes nothing.
    #[schema(format = DateTime, required = true)]
    pub start_at: Option<String>,
    /// RFC 3339, UTC. Shown to learners and staff; it opens and closes nothing.
    #[schema(format = DateTime, required = true)]
    pub end_at: Option<String>,
    pub settings: CourseSettings,
    /// The catalog keys grantable at course scope that you hold at this course, sorted: what
    /// the app may offer you here. The server checks every request itself.
    pub permissions: Vec<String>,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub updated_at: String,
}

impl Course {
    fn from_repo(course: courses::Course, grants: &Grants) -> Result<Self, ApiError> {
        let time = |t| timestamp::rfc3339(t).map_err(|error| ApiError::internal(&error));
        let date = |t: Option<SystemTime>| t.map(time).transpose();
        let target = Target::Course {
            account_path: course.account_path.clone(),
            course_id: course.id,
        };
        Ok(Self {
            permissions: course_permissions(grants, &target),
            start_at: date(course.start_at)?,
            end_at: date(course.end_at)?,
            created_at: time(course.created_at)?,
            updated_at: time(course.updated_at)?,
            settings: CourseSettings {
                home: course.settings.home.into(),
            },
            status: course.status.into(),
            id: course.id,
            account_id: course.account_id,
            title: course.title,
            code: course.code,
            template: course.template,
        })
    }
}

/// The catalog keys grantable at course scope that `grants` hold at `target`, sorted (B10).
fn course_permissions(grants: &Grants, target: &Target) -> Vec<String> {
    let mut keys: Vec<String> = perm::ALL
        .iter()
        .filter(|p| p.grantable().contains(&ScopeType::Course))
        .filter(|p| grants.holds_key(p.key(), target))
        .map(|p| p.key().to_owned())
        .collect();
    keys.sort_unstable();
    keys
}

/// A course to create, as a draft.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewCourse {
    /// The account the course belongs to; you must hold `course.create` at a scope containing
    /// it.
    #[schema(value_type = String, format = "uuid")]
    pub account_id: Uuid,
    /// Trimmed; 1 to 200 characters.
    pub title: String,
    /// Trimmed; 1 to 50 characters; unique in the organization, ignoring ASCII case.
    pub code: String,
}

/// A date sent by the client: an RFC 3339 timestamp with an offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiTimestamp(SystemTime);

impl ApiTimestamp {
    /// The instant the timestamp names.
    pub fn time(self) -> SystemTime {
        self.0
    }
}

impl<'de> Deserialize<'de> for ApiTimestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        courses::parse_timestamp(&text)
            .map(Self)
            .ok_or_else(|| serde::de::Error::custom("not an RFC 3339 timestamp with an offset"))
    }
}

/// A member that may be `null`: present, it reads as `Some(value)` with `value` `None` for
/// `null`; omitted, it reads as `None` through `#[serde(default)]`.
pub(crate) fn nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// A change to a course's settings; an omitted member is left as it is.
#[derive(Clone, Copy, Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CourseSettingsPatch {
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = CourseHome)]
    pub home: Option<CourseHome>,
}

/// A change to a course; an omitted member is left as it is. `null` clears `start_at` or
/// `end_at` and is malformed for every other member.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CoursePatch {
    /// Trimmed; 1 to 200 characters.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub title: Option<String>,
    /// Trimmed; 1 to 50 characters; unique in the organization, ignoring ASCII case.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub code: Option<String>,
    /// RFC 3339 with an offset, from 1970 up to 2200; `null` clears it.
    #[serde(default, deserialize_with = "nullable")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub start_at: Option<Option<ApiTimestamp>>,
    /// RFC 3339 with an offset, from 1970 up to 2200, not before `start_at`; `null` clears it.
    #[serde(default, deserialize_with = "nullable")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub end_at: Option<Option<ApiTimestamp>>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = CourseSettingsPatch)]
    pub settings: Option<CourseSettingsPatch>,
}

impl CoursePatch {
    fn repo(&self) -> courses::CoursePatch {
        let date = |d: Option<Option<ApiTimestamp>>| d.map(|d| d.map(|ApiTimestamp(t)| t));
        courses::CoursePatch {
            title: self.title.clone(),
            code: self.code.clone(),
            start_at: date(self.start_at),
            end_at: date(self.end_at),
            home: self.settings.and_then(|s| s.home).map(Into::into),
        }
    }
}

/// The status to change a course to.
#[derive(Clone, Copy, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CourseStatusChange {
    pub status: CourseStatus,
}

/// The account to move a course to.
#[derive(Clone, Copy, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CourseMove {
    /// You must hold `course.create` at a scope containing it. The course's own account
    /// changes nothing.
    #[schema(value_type = String, format = "uuid")]
    pub account_id: Uuid,
}

/// A copy to make of a course, as a draft.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CourseCopy {
    /// The account the copy belongs to; you must hold `course.create` at a scope containing it.
    #[schema(value_type = String, format = "uuid")]
    pub account_id: Uuid,
    /// Trimmed; 1 to 200 characters. The source's title when omitted.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub title: Option<String>,
    /// Trimmed; 1 to 50 characters; unique in the organization, ignoring ASCII case.
    pub code: String,
    /// Whether the copy is a template. Omitted or `false`, the copy is an ordinary course, also
    /// when the source is a template.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = bool)]
    pub template: Option<bool>,
}

/// The filters of the courses list.
#[derive(Clone, Copy, Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CourseListParams {
    /// Only courses in this account's subtree.
    #[param(value_type = Option<String>, format = "uuid")]
    pub account_id: Option<Uuid>,
    #[param(value_type = Option<CourseStatus>)]
    pub status: Option<CourseStatus>,
    pub template: Option<bool>,
}

/// List the courses where you hold `course.view`, drafts and templates only where you also hold
/// `course.view_all`, sorted by title, then id.
#[utoipa::path(
    get,
    path = "/api/v1/courses",
    operation_id = "listCourses",
    tag = "courses",
    params(CourseListParams, PageParams),
    responses(
        (status = 200, description = "One page of courses", body = Page<Course>),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = CourseListNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_courses(
    require: Require<CourseView, AnyScope>,
    State(state): State<AppState>,
    ApiQuery(filter): ApiQuery<CourseListParams>,
    ApiQuery(params): ApiQuery<PageParams>,
) -> Result<Json<Page<Course>>, ApiError> {
    let page = params.parse::<courses::ListKey>()?;
    let grants = require.grants();
    if let Some(account_id) = filter.account_id {
        check_visible_account(&state, grants, account_id).await?;
    }
    let (view, view_all) = (reach::<CourseView>(grants), reach::<CourseViewAll>(grants));
    let visibility = courses::Visibility {
        view: as_reach(&view),
        view_all: as_reach(&view_all),
    };
    let filter = courses::ListFilter {
        account_id: filter.account_id,
        status: filter.status.map(Into::into),
        template: filter.template,
    };
    let (after, limit) = (page.after(), page.fetch_limit());
    let rows = courses::list_visible(&state.db, grants.scope(), visibility, filter, after, limit)
        .await
        .map_err(|error| match error {
            // Only the cursor's title is a caller's string here.
            courses::Error::ContainsNul { .. } => ApiError::invalid_cursor(),
            other => course_error(other),
        })?;
    let rows = rows
        .into_iter()
        .map(|course| Course::from_repo(course, grants))
        .collect::<Result<Vec<_>, _>>()?;
    page.page(rows, |course| (course.title.clone(), course.id))
        .map(Json)
}

/// 404 `account.not_found` unless the account exists and the caller holds a permission at a
/// scope containing it (B11's `account_id` filter).
async fn check_visible_account(
    state: &AppState,
    grants: &Grants,
    account_id: Uuid,
) -> Result<(), ApiError> {
    let path = accounts::ancestors_path(&state.db, grants.scope(), account_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or(Denied::AccountNotFound)?;
    if grants.is_visible(&Target::Account(path)) {
        Ok(())
    } else {
        Err(Denied::AccountNotFound.into())
    }
}

/// Where `P` is effective: `None` for the whole organization, otherwise the accounts.
fn reach<P: Permission>(grants: &Grants) -> Option<Vec<Uuid>> {
    let mut ids = Vec::new();
    for scope in grants.scopes_with::<P>() {
        match scope {
            ScopeId::Organization(_) => return None,
            ScopeId::Account(id) => ids.push(id),
        }
    }
    Some(ids)
}

fn as_reach(reach: &Option<Vec<Uuid>>) -> accounts::Reach<'_> {
    match reach {
        None => accounts::Reach::Organization,
        Some(ids) => accounts::Reach::Accounts(ids),
    }
}

/// Read one course.
#[utoipa::path(
    get,
    path = "/api/v1/courses/{course_id}",
    operation_id = "getCourse",
    tag = "courses",
    params(("course_id" = String, Path, format = "uuid", description = "The course's id.")),
    responses(
        (status = 200, description = "The course", body = Course),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = CourseNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_course(
    require: Require<CourseView, CourseTarget>,
    State(state): State<AppState>,
) -> Result<Json<Course>, ApiError> {
    let grants = require.grants();
    let course = courses::get(&state.db, grants.scope(), require.target().id)
        .await
        .map_err(course_error)?
        .ok_or(Denied::CourseNotFound)?;
    // Again on the row read here: a course made a draft after the extractor ran stays hidden,
    // and its body is not returned.
    let target = CourseTarget {
        id: course.id,
        account_path: course.account_path.clone(),
    };
    check_course::<CourseView>(grants, &target, course.needs_view_all(), COURSE_NOT_FOUND)?;
    Ok(Json(Course::from_repo(course, grants)?))
}

/// Create a draft course in `account_id`.
#[utoipa::path(
    post,
    path = "/api/v1/courses",
    operation_id = "createCourse",
    tag = "courses",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = NewCourse,
    responses(
        (
            status = 201,
            description = "The course is created",
            body = Course,
            headers(("Location" = String, description = "The course's path"))
        ),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = CourseCreateForbidden),
        (status = 404, response = CourseCreateNotFound),
        (status = 409, response = CourseCodeConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn create_course(
    require: Require<CourseCreate, AnyScope>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<NewCourse>,
) -> Result<Response, ApiError> {
    let grants = require.grants();
    let mut tx = locked_tree(&state, grants.scope()).await?;
    grants
        .require_at::<CourseCreate>(&mut *tx, ScopeId::Account(body.account_id))
        .await?;
    let new = courses::NewCourse {
        account_id: body.account_id,
        title: body.title,
        code: body.code,
    };
    let actor = client.actor(require.principal());
    let course = courses::create(&mut *tx, grants.scope(), &new, state.clock.now(), actor)
        .await
        .map_err(course_error)?;
    commit(tx).await?;
    let location = format!("{COURSES_PATH}/{}", course.id);
    let course = Course::from_repo(course, grants)?;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(course),
    )
        .into_response())
}

/// Change a course's title, code, dates or home. A patch naming only current values changes
/// nothing.
#[utoipa::path(
    patch,
    path = "/api/v1/courses/{course_id}",
    operation_id = "updateCourse",
    tag = "courses",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("course_id" = String, Path, format = "uuid", description = "The course's id.")),
    request_body = CoursePatch,
    responses(
        (status = 200, description = "The course after the change", body = Course),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = CourseManageForbidden),
        (status = 404, response = CourseNotFound),
        (status = 409, response = CourseCodeConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_course(
    require: Require<CourseManage, CourseTarget>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<CoursePatch>,
) -> Result<Json<Course>, ApiError> {
    let (grants, id) = (require.grants(), require.target().id);
    let mut tx = begin(&state).await?;
    recheck_manage(&mut tx, grants, id).await?;
    let actor = client.actor(require.principal());
    let course = courses::update(
        &mut *tx,
        grants.scope(),
        id,
        &body.repo(),
        state.clock.now(),
        actor,
    )
    .await
    .map_err(course_error)?;
    commit(tx).await?;
    Ok(Json(Course::from_repo(course, grants)?))
}

/// Change a course's status. Asking for the current status changes nothing.
#[utoipa::path(
    post,
    path = "/api/v1/courses/{course_id}/status",
    operation_id = "setCourseStatus",
    tag = "courses",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("course_id" = String, Path, format = "uuid", description = "The course's id.")),
    request_body = CourseStatusChange,
    responses(
        (status = 200, description = "The course after the change", body = Course),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = CourseManageForbidden),
        (status = 404, response = CourseNotFound),
        (status = 409, response = CourseStatusConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn set_course_status(
    require: Require<CourseManage, CourseTarget>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<CourseStatusChange>,
) -> Result<Json<Course>, ApiError> {
    let (grants, id) = (require.grants(), require.target().id);
    let mut tx = begin(&state).await?;
    recheck_manage(&mut tx, grants, id).await?;
    let (status, now) = (body.status.into(), state.clock.now());
    let actor = client.actor(require.principal());
    let course = courses::set_status(&mut *tx, grants.scope(), id, status, now, actor)
        .await
        .map_err(course_error)?;
    commit(tx).await?;
    Ok(Json(Course::from_repo(course, grants)?))
}

/// Move a course, in any status, to another account of the organization.
#[utoipa::path(
    post,
    path = "/api/v1/courses/{course_id}/move",
    operation_id = "moveCourse",
    tag = "courses",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("course_id" = String, Path, format = "uuid", description = "The course's id.")),
    request_body = CourseMove,
    responses(
        (status = 200, description = "The course after the move", body = Course),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = CourseMoveForbidden),
        (status = 404, response = CourseMoveNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn move_course(
    require: Require<CourseManage, CourseTarget>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<CourseMove>,
) -> Result<Json<Course>, ApiError> {
    let (grants, id) = (require.grants(), require.target().id);
    let mut tx = locked_tree(&state, grants.scope()).await?;
    let from = recheck_manage(&mut tx, grants, id).await?;
    let destination = creatable_destination(&mut tx, grants, body.account_id).await?;
    let to = Target::Course {
        account_path: destination,
        course_id: id,
    };
    let detail = "The destination would give you permissions over this course that you do not hold over it now.";
    refuse_escalation(grants, &from, &to, detail)?;
    let (now, actor) = (state.clock.now(), client.actor(require.principal()));
    let moved = courses::move_to(&mut *tx, grants.scope(), id, body.account_id, now, actor)
        .await
        .map_err(course_error)?;
    commit(tx).await?;
    Ok(Json(Course::from_repo(moved, grants)?))
}

/// Copy a course into an account as a new draft, or as a template.
#[utoipa::path(
    post,
    path = "/api/v1/courses/{course_id}/copy",
    operation_id = "copyCourse",
    tag = "courses",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("course_id" = String, Path, format = "uuid", description = "The source course's id.")),
    request_body = CourseCopy,
    responses(
        (
            status = 201,
            description = "The copy is created",
            body = Course,
            headers(("Location" = String, description = "The copy's path"))
        ),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = CourseCopyForbidden),
        (status = 404, response = CourseCopyNotFound),
        (status = 409, response = CourseCodeConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn copy_course(
    require: Require<CourseViewAll, CourseTarget>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<CourseCopy>,
) -> Result<Response, ApiError> {
    let (grants, id) = (require.grants(), require.target().id);
    let mut tx = locked_tree(&state, grants.scope()).await?;
    let from = recheck_view_all(&mut tx, grants, id).await?;
    let destination = creatable_destination(&mut tx, grants, body.account_id).await?;
    let to = Target::Course {
        account_path: destination,
        course_id: id,
    };
    let detail = "The destination would give you permissions over the copy that you do not hold over this course.";
    refuse_escalation(grants, &from, &to, detail)?;
    let copy = courses::CourseCopy {
        account_id: body.account_id,
        title: body.title,
        code: body.code,
        template: body.template.unwrap_or(false),
    };
    let (now, actor) = (state.clock.now(), client.actor(require.principal()));
    let course = courses::copy(&mut *tx, grants.scope(), id, &copy, now, actor)
        .await
        .map_err(course_error)?;
    commit(tx).await?;
    let location = format!("{COURSES_PATH}/{}", course.id);
    let course = Course::from_repo(course, grants)?;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(course),
    )
        .into_response())
}

/// The path of the account `account_id`, read in the transaction under the tree lock, when the
/// caller holds `course.create` there; otherwise 404 `account.not_found`, as for an account
/// that does not exist (B9, B12).
async fn creatable_destination(
    tx: &mut Transaction<'static, Postgres>,
    grants: &Grants,
    account_id: Uuid,
) -> Result<Vec<Uuid>, ApiError> {
    let destination = accounts::ancestors_path(&mut **tx, grants.scope(), account_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or(Denied::AccountNotFound)?;
    if !grants.holds::<CourseCreate>(&Target::Account(destination.clone())) {
        return Err(Denied::AccountNotFound.into());
    }
    Ok(destination)
}

/// The extractor's `course.view_all` check again, on the course's path as the transaction reads
/// it under the tree lock, which a course or account move also takes: 404 when the course is
/// gone or out of the caller's reach, 403 when it is visible without `course.view_all`. Returns
/// the course as the evaluator's target.
async fn recheck_view_all(
    tx: &mut Transaction<'static, Postgres>,
    grants: &Grants,
    id: Uuid,
) -> Result<Target, ApiError> {
    let account_path = courses::path(&mut **tx, grants.scope(), id)
        .await
        .map_err(course_error)?
        .ok_or(Denied::CourseNotFound)?;
    let target = Target::Course {
        account_path,
        course_id: id,
    };
    grants.check::<CourseViewAll>(&target)?;
    Ok(target)
}

/// The extractor's `course.manage` check again ([`check_course`]), on the course as
/// `courses::lock` reads it in the transaction: 404 when the course is gone, out of the caller's
/// reach, or a draft or template without `course.view_all` (Q13); 403 when it is visible without
/// `course.manage`. Returns the course as the evaluator's target.
async fn recheck_manage(
    tx: &mut Transaction<'static, Postgres>,
    grants: &Grants,
    id: Uuid,
) -> Result<Target, ApiError> {
    let course = courses::lock(tx, grants.scope(), id)
        .await
        .map_err(course_error)?
        .ok_or(Denied::CourseNotFound)?;
    let needs_view_all = course.needs_view_all();
    let course = CourseTarget {
        id,
        account_path: course.account_path,
    };
    check_course::<CourseManage>(grants, &course, needs_view_all, COURSE_NOT_FOUND)?;
    Ok(course.target())
}

/// 403 `permission.escalation`, with `detail`, when the course at `to` would give the caller a
/// permission over it, among those grantable at course scope, that they do not hold over it at
/// `from`, as an account move refuses (m0 spec 02 B23): every grant on the destination's path
/// contains a course moved or copied there. Permissions not grantable at course scope, such as
/// `course.create`, which a move or copy already needs at the destination, are left out (owner
/// decision, 2026-11-16).
fn refuse_escalation(
    grants: &Grants,
    from: &Target,
    to: &Target,
    detail: &'static str,
) -> Result<(), ApiError> {
    let gained = perm::ALL
        .iter()
        .filter(|p| p.grantable().contains(&ScopeType::Course))
        .any(|p| grants.holds_key(p.key(), to) && !grants.holds_key(p.key(), from));
    if !gained {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::FORBIDDEN,
        PERMISSION_ESCALATION,
        detail,
    ))
}

async fn begin(state: &AppState) -> Result<Transaction<'static, Postgres>, ApiError> {
    state
        .db
        .begin()
        .await
        .map_err(|error| ApiError::internal(&error))
}

/// A transaction holding the scope's account tree lock, for checks that must still hold at the
/// write.
async fn locked_tree(
    state: &AppState,
    scope: OrgScope,
) -> Result<Transaction<'static, Postgres>, ApiError> {
    let mut tx = begin(state).await?;
    accounts::lock_tree(&mut tx, scope)
        .await
        .map_err(|error| ApiError::internal(&error))?;
    Ok(tx)
}

async fn commit(tx: Transaction<'static, Postgres>) -> Result<(), ApiError> {
    tx.commit()
        .await
        .map_err(|error| ApiError::internal(&error))
}

/// The answer for a courses repository error.
fn course_error(error: courses::Error) -> ApiError {
    use courses::Error as E;
    let invalid = |field: &str, code| ApiError::invalid(vec![FieldError::at(&[field], code)]);
    let conflict = |code, detail| ApiError::new(StatusCode::CONFLICT, code, detail);
    let date_field = |field| match field {
        DateField::StartAt => "start_at",
        DateField::EndAt => "end_at",
    };
    match error {
        E::NotFound => Denied::CourseNotFound.into(),
        E::AccountNotFound => Denied::AccountNotFound.into(),
        E::TitleEmpty => invalid("title", NAME_EMPTY),
        E::TitleTooLong => invalid("title", NAME_TOO_LONG),
        E::CodeEmpty => invalid("code", NAME_EMPTY),
        E::CodeTooLong => invalid("code", TEXT_TOO_LONG),
        E::ContainsNul { field } => invalid(field, TEXT_CONTAINS_NUL),
        E::DatesInverted => invalid("end_at", code::COURSE_DATES_INVERTED),
        E::DateOutOfRange { field } => invalid(date_field(field), code::COURSE_DATE_OUT_OF_RANGE),
        E::CodeTaken => conflict(
            code::COURSE_CODE_TAKEN,
            "Another course of the organization has this code.",
        ),
        E::StatusInvalid { .. } => conflict(
            code::COURSE_STATUS_INVALID,
            "The course cannot change from its status to this one.",
        ),
        E::Template => conflict(code::COURSE_TEMPLATE, "A template cannot be published."),
        other @ (E::Stored { .. }
        | E::ContentNotCopyable { .. }
        | E::CopiedUrlInvalid { .. }
        | E::Account(_)
        | E::Time(_)
        | E::Database(_)) => ApiError::internal(&other),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use stepclimb_core::{
        id::new_id,
        role_assignments::{GrantScope, UserGrant},
        roles,
    };

    use super::*;

    #[expect(
        clippy::disallowed_methods,
        reason = "unit tests of the permissions list need grants and have no credential"
    )]
    fn grants(role: &str, scope: GrantScope) -> Grants {
        let grant = UserGrant {
            assignment_id: new_id(),
            role_id: new_id(),
            role_key: role.into(),
            permissions: roles::builtin(role)
                .expect("a built-in role")
                .permissions()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            scope,
        };
        Grants::from_assignments(OrgScope::instance(new_id()), [grant])
    }

    #[test]
    fn the_permissions_list_holds_only_course_grantable_keys() {
        let tower = new_id();
        let course = Target::Course {
            account_path: vec![tower],
            course_id: new_id(),
        };
        let at_tower = GrantScope::Account {
            id: tower,
            path: vec![tower],
        };
        assert_eq!(
            course_permissions(&grants("course_designer", at_tower.clone()), &course),
            ["course.content_edit", "course.view", "course.view_all"]
        );
        assert_eq!(
            course_permissions(&grants("org_admin", GrantScope::Organization), &course),
            [
                "course.content_edit",
                "course.manage",
                "course.view",
                "course.view_all",
                "enrollment.manage",
                "enrollment.view",
            ],
            "no organization or account permission"
        );
        let elsewhere = GrantScope::Account {
            id: new_id(),
            path: vec![new_id()],
        };
        assert!(course_permissions(&grants("account_admin", elsewhere), &course).is_empty());
    }

    #[test]
    fn repository_errors_map_to_their_status_code_and_field() {
        use courses::Error as E;
        let cases = [
            (E::NotFound, 404, "course.not_found", None),
            (E::AccountNotFound, 404, "account.not_found", None),
            (E::CodeTaken, 409, "course.code_taken", None),
            (
                E::StatusInvalid {
                    from: "draft",
                    to: "archived",
                },
                409,
                "course.status_invalid",
                None,
            ),
            (E::Template, 409, "course.template", None),
            (
                E::TitleEmpty,
                422,
                "request.invalid",
                Some(("/title", "name.empty")),
            ),
            (
                E::TitleTooLong,
                422,
                "request.invalid",
                Some(("/title", "name.too_long")),
            ),
            (
                E::CodeEmpty,
                422,
                "request.invalid",
                Some(("/code", "name.empty")),
            ),
            (
                E::CodeTooLong,
                422,
                "request.invalid",
                Some(("/code", "text.too_long")),
            ),
            (
                E::DatesInverted,
                422,
                "request.invalid",
                Some(("/end_at", "course.dates_inverted")),
            ),
            (
                E::DateOutOfRange {
                    field: DateField::StartAt,
                },
                422,
                "request.invalid",
                Some(("/start_at", "course.date_out_of_range")),
            ),
            (
                E::ContainsNul { field: "code" },
                422,
                "request.invalid",
                Some(("/code", "text.contains_nul")),
            ),
        ];
        for (error, status, code, field) in cases {
            let problem = course_error(error).problem();
            assert_eq!((problem.status, problem.code.as_str()), (status, code));
            let fields: Vec<_> = problem
                .errors
                .iter()
                .map(|e| (e.field.as_str(), e.code.as_str()))
                .collect();
            assert_eq!(fields, field.into_iter().collect::<Vec<_>>(), "{code}");
        }
    }

    #[test]
    fn a_patch_takes_null_only_for_dates() {
        for refused in [
            json!({ "title": null }),
            json!({ "code": null }),
            json!({ "settings": null }),
            json!({ "settings": { "home": null } }),
            json!({ "settings": { "home": { "kind": "page" } } }),
            json!({ "start_at": "2026-01-02" }),
            json!({ "status": "published" }),
        ] {
            assert!(
                serde_json::from_value::<CoursePatch>(refused.clone()).is_err(),
                "{refused}"
            );
        }
        let cleared: CoursePatch =
            serde_json::from_value(json!({ "start_at": null, "settings": {} })).unwrap();
        let repo = cleared.repo();
        assert_eq!(repo.start_at, Some(None));
        assert_eq!((repo.end_at, repo.home), (None, None));
        let set: CoursePatch = serde_json::from_value(json!({
            "end_at": "2026-01-02T03:04:05Z",
            "settings": { "home": { "kind": "modules" } },
        }))
        .unwrap();
        assert!(matches!(set.repo().end_at, Some(Some(_))));
        assert_eq!(set.repo().home, Some(courses::Home::Modules));
    }
}
