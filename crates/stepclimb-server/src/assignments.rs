// SPDX-License-Identifier: AGPL-3.0-or-later
//! Assignments: `/api/v1/courses/{course_id}/assignments` and `/api/v1/assignments/{id}`
//! (docs/specs/m1/13-assignments-due-dates-reminders-and-escalation.md B1 to B5, B7 to B9 and
//! API), and the subscribers that complete assignees' rows from the progress events (B8).
//!
//! An assignment says who must finish a course, or a module or item of it, and by when. Every
//! route needs `assignment.manage` at the course, which the caller may hold at the
//! organization, an account containing the course, or through an enrollment in it (Q8).
//! Listing a course's assignments and creating one also need `enrollment.view` there (403
//! `permission.denied`): an assignment names its assignee, so `assignment.manage` alone would
//! show who is in the course.
//!
//! `POST` assigns one user (group assignees are spec 13 I5). In the write's transaction, after
//! `courses::lock` and the `assignment.manage` check again on the path it read, an archived
//! course or a template answers 409. A user who is not a learner of the course (an active
//! enrollment whose role holds `course.participate`) is enrolled as Student in the same
//! transaction (B3, Q2), which needs what `POST /api/v1/courses/{course_id}/enrollments` needs
//! for that role: the user covered by the caller's `user.view` (404 `user.not_found`
//! otherwise), `enrollment.manage` at the course (403 `permission.denied`), and every Student
//! permission grantable at organization or account scope held at the course (403
//! `permission.escalation`). The reach check reads through the write's transaction, so no second
//! pool connection is taken while the course row is locked. Assigning a learner needs none of
//! them: an Instructor assigns the students of their course with their enrollment's permissions
//! alone, and gets 404 for a user outside it, since `user.view` is not grantable at a course.
//!
//! `PATCH` replaces the due rule and recomputes the due date of every row not completed (B5);
//! an ended assignment answers 409 `assignment.ended`. `POST .../end` ends it, keeping its rows
//! (Q3); ending it again changes nothing.

use std::time::SystemTime;

use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction, pool::PoolConnection};
use stepclimb_core::{
    assignments::{self as repo, Completed, DueError, DueRule, Enroll, NewAssignment, TargetType},
    courses,
    grants::{Denied, Grants, Target},
    id::Uuid,
    perm::{AssignmentManage, EnrollmentManage, EnrollmentView},
    roles,
};
use stepclimb_jobs::{JobContext, Kind, Outcome, Registry, RegistryError, outbox};
use utoipa::{IntoParams, ToSchema};

use crate::{
    AppState,
    audit::ClientAddress,
    course_enrollments::{refuse_escalation, refuse_unreachable_user},
    courses::{
        code::{COURSE_ARCHIVED, COURSE_TEMPLATE},
        nullable,
    },
    error::{ApiError, FieldError},
    extract::{ApiJson, ApiQuery},
    openapi::problem_responses,
    pagination::{Page, PageParams},
    progress::{COURSE_COMPLETED, ITEM_COMPLETED, MODULE_COMPLETED},
    require::{
        AssignmentTarget, CourseTarget, Require, check_course,
        code::{
            ASSIGNMENT_NOT_FOUND, COURSE_NOT_FOUND, ITEM_NOT_FOUND, MODULE_NOT_FOUND,
            PERMISSION_DENIED,
        },
        not_found_as,
    },
    role_assignments::code::USER_NOT_FOUND,
    timestamp,
};

/// The path an assignment is at: `{ASSIGNMENT_PATH}/{assignment_id}`.
pub const ASSIGNMENT_PATH: &str = "/api/v1/assignments";

/// The earliest fixed due date: the Unix epoch, as the database's times are written.
const FIXED_DUE_MIN_SECS: u64 = 0;

/// The end of the last year a fixed due date may fall in: 2200-01-01T00:00:00Z, as a module's
/// `unlock_at` (spec 03).
const FIXED_DUE_END_SECS: u64 = 7_258_118_400;

/// The codes this module produces besides `course.*`, `module.not_found`, `item.not_found`,
/// `user.not_found` and `assignment.not_found` (`require::code`).
pub mod code {
    /// The assignment has ended and takes no change (409).
    pub const ASSIGNMENT_ENDED: &str = "assignment.ended";
    /// In a 422's `errors`: the due rule names both or neither of `fixed` and `relative_days`,
    /// a fixed time outside 1970 to 2199, or a number of days outside 1 to 3650.
    pub const ASSIGNMENT_DUE_INVALID: &str = "assignment.due_invalid";
    /// In a 422's `errors`: a `course` target names a course other than the path's.
    pub const ASSIGNMENT_TARGET_MISMATCH: &str = "assignment.target_mismatch";
    /// In a 422's `errors`: a `group` assignee, which this release does not take yet.
    pub const ASSIGNMENT_ASSIGNEE_UNAVAILABLE: &str = "assignment.assignee_unavailable";
    /// The assignee's concluded Student enrollment is owned by an outside system (409).
    pub const ENROLLMENT_EXTERNALLY_MANAGED: &str = "enrollment.externally_managed";
}

problem_responses! {
    AssignmentListNotFound => "`course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined.",
    AssignmentCreateNotFound => "`course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `module.not_found`, `item.not_found`: the target is no module or item of this course. `user.not_found`: no user of the organization has this id, or, for a user who is not a learner of the course, your `user.view` does not cover them. `org.not_found`: the request's organization cannot be determined.",
    AssignmentListForbidden => "`permission.denied`: you can see the course but do not hold both `assignment.manage` and `enrollment.view` at scopes containing it.",
    AssignmentCreateForbidden => "`permission.denied`: you can see the course but do not hold both `assignment.manage` and `enrollment.view` at scopes containing it, or the user is not a learner of the course, so assigning them enrolls them as Student, and you do not hold `enrollment.manage` there. `permission.escalation`: enrolling the user as Student needs every Student permission grantable at organization or account scope, held at the course. `auth.csrf_refused`: the request did not come from this site.",
    AssignmentCreateConflict => "`course.archived`: an archived course takes no assignment. `course.template`: a template takes no assignment. `enrollment.externally_managed`: the user's concluded Student enrollment in the course is owned by an identity provider, the sync API, SCIM or a connector, so the assignment cannot reactivate it.",
    AssignmentNotFound => "`assignment.not_found`: no assignment with this id exists in the organization, or you cannot see its course; a draft course or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined.",
    AssignmentWriteForbidden => "`permission.denied`: you can see the assignment's course but do not hold `assignment.manage` at a scope containing it. `auth.csrf_refused`: the request did not come from this site.",
    AssignmentUpdateConflict => "`assignment.ended`: the assignment has ended and takes no change.",
}

/// What an assignment names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentTargetType {
    Course,
    Module,
    Item,
}

impl From<AssignmentTargetType> for TargetType {
    fn from(target: AssignmentTargetType) -> Self {
        match target {
            AssignmentTargetType::Course => Self::Course,
            AssignmentTargetType::Module => Self::Module,
            AssignmentTargetType::Item => Self::Item,
        }
    }
}

impl From<TargetType> for AssignmentTargetType {
    fn from(target: TargetType) -> Self {
        match target {
            TargetType::Course => Self::Course,
            TargetType::Module => Self::Module,
            TargetType::Item => Self::Item,
        }
    }
}

/// Who an assignment reaches. `group` is refused until group assignees exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssigneeType {
    User,
    Group,
}

/// When an assignment is due: exactly one of `fixed` and `relative_days`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AssignmentDue {
    /// RFC 3339 with an offset, from 1970 up to 2200: the same due time for everyone. Answered
    /// in UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String, format = DateTime, required = false)]
    pub fixed: Option<String>,
    /// 1 to 3650: each person is due that many calendar days after they were assigned, at
    /// 23:59:59 local time of the last day in the organization's default time zone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = u16, minimum = 1, required = false)]
    pub relative_days: Option<u32>,
}

impl AssignmentDue {
    /// The rule this names, or the 422 that refuses it at `/due`.
    fn rule(&self) -> Result<DueRule, ApiError> {
        let invalid = |at: &[&str]| {
            let mut path = vec!["due"];
            path.extend_from_slice(at);
            ApiError::invalid(vec![FieldError::at(&path, code::ASSIGNMENT_DUE_INVALID)])
        };
        match (&self.fixed, self.relative_days) {
            (Some(text), None) => {
                let time = courses::parse_timestamp(text).ok_or_else(|| invalid(&["fixed"]))?;
                if !fixed_in_range(time) {
                    return Err(invalid(&["fixed"]));
                }
                Ok(DueRule::Fixed(time))
            }
            (None, Some(days)) => u16::try_from(days)
                .ok()
                .filter(|days| (1..=repo::MAX_RELATIVE_DAYS).contains(days))
                .map(DueRule::RelativeDays)
                .ok_or_else(|| invalid(&["relative_days"])),
            _ => Err(invalid(&[])),
        }
    }

    fn from_rule(rule: DueRule) -> Result<Self, ApiError> {
        Ok(match rule {
            DueRule::Fixed(at) => Self {
                fixed: Some(time(at)?),
                relative_days: None,
            },
            DueRule::RelativeDays(days) => Self {
                fixed: None,
                relative_days: Some(u32::from(days)),
            },
        })
    }
}

fn fixed_in_range(time: SystemTime) -> bool {
    let secs = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|since| since.as_secs());
    secs.is_ok_and(|secs| (FIXED_DUE_MIN_SECS..FIXED_DUE_END_SECS).contains(&secs))
}

/// The target of a new assignment: the path's course, or a module or item of it.
#[derive(Clone, Copy, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewAssignmentTarget {
    #[serde(rename = "type")]
    pub target_type: AssignmentTargetType,
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
}

/// Who a new assignment reaches.
#[derive(Clone, Copy, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewAssignee {
    /// `user`; `group` is refused until group assignees exist.
    #[serde(rename = "type")]
    pub assignee_type: AssigneeType,
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
}

/// An assignment to create.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewAssignmentBody {
    pub target: NewAssignmentTarget,
    pub assignee: NewAssignee,
    /// Omitted or `null`: no due date, and the assignment is never overdue.
    #[serde(default)]
    #[schema(nullable = true)]
    pub due: Option<AssignmentDue>,
}

/// A change to an assignment; an omitted member is left as it is.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AssignmentPatch {
    /// The new due rule; `null` removes the due date. Every row not completed gets its due date
    /// again from it.
    #[serde(default, deserialize_with = "nullable")]
    #[schema(value_type = Option<AssignmentDue>)]
    pub due: Option<Option<AssignmentDue>>,
}

/// An assignment's target, with its title.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct AssignmentTargetRef {
    #[serde(rename = "type")]
    pub target_type: AssignmentTargetType,
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub title: String,
}

/// An assignment's assignee, with their name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct AssigneeRef {
    #[serde(rename = "type")]
    pub assignee_type: AssigneeType,
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub name: String,
}

/// How many of an assignment's people are in each status, at the time of the request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct AssignmentCounts {
    pub not_started: u32,
    pub in_progress: u32,
    pub completed: u32,
    pub overdue: u32,
}

/// Who must finish which course, module or item, and by when.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Assignment {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub course_id: Uuid,
    pub target: AssignmentTargetRef,
    pub assignee: AssigneeRef,
    /// `null` when the assignment has no due date.
    #[schema(required = true)]
    pub due: Option<AssignmentDue>,
    /// RFC 3339, UTC; `null` while the assignment runs.
    #[schema(format = DateTime, required = true)]
    pub ended_at: Option<String>,
    pub counts: AssignmentCounts,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
}

fn time(at: SystemTime) -> Result<String, ApiError> {
    timestamp::rfc3339(at).map_err(|error| ApiError::internal(&error))
}

impl Assignment {
    fn from_repo(a: repo::Assignment) -> Result<Self, ApiError> {
        let counts = a.counts;
        Ok(Self {
            id: a.id,
            course_id: a.course_id,
            target: AssignmentTargetRef {
                target_type: a.target_type.into(),
                id: a.target_id,
                title: a.target_title,
            },
            assignee: AssigneeRef {
                assignee_type: AssigneeType::User,
                id: a.user_id,
                name: a.user_display_name,
            },
            due: a.due.map(AssignmentDue::from_rule).transpose()?,
            ended_at: a.ended_at.map(time).transpose()?,
            counts: AssignmentCounts {
                not_started: counts.not_started,
                in_progress: counts.in_progress,
                completed: counts.completed,
                overdue: counts.overdue,
            },
            created_at: time(a.created_at)?,
        })
    }
}

/// The filter of a course's assignments.
#[derive(Clone, Copy, Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AssignmentListParams {
    /// Also list ended assignments; `false` when omitted.
    pub include_ended: Option<bool>,
}

/// List a course's assignments, newest first. Needs `enrollment.view` at the course too, since
/// each assignment names its assignee.
#[utoipa::path(
    get,
    path = "/api/v1/courses/{course_id}/assignments",
    operation_id = "listAssignments",
    tag = "assignments",
    params(
        ("course_id" = String, Path, format = "uuid", description = "The course's id."),
        AssignmentListParams,
        PageParams,
    ),
    responses(
        (status = 200, description = "One page of assignments", body = Page<Assignment>),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = AssignmentListForbidden),
        (status = 404, response = AssignmentListNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_assignments(
    require: Require<AssignmentManage, CourseTarget>,
    State(state): State<AppState>,
    ApiQuery(filter): ApiQuery<AssignmentListParams>,
    ApiQuery(params): ApiQuery<PageParams>,
) -> Result<Json<Page<Assignment>>, ApiError> {
    let page = params.parse::<Uuid>()?;
    refuse_without_roster(require.grants(), &require.target().target())?;
    let (scope, course_id) = (require.grants().scope(), require.target().id);
    let mut conn = acquire(&state).await?;
    let rows = repo::list_for_course(
        &mut conn,
        scope,
        course_id,
        filter.include_ended.unwrap_or(false),
        page.after().copied(),
        page.fetch_limit(),
        state.clock.now(),
    )
    .await
    .map_err(assignment_error)?;
    let rows = rows
        .into_iter()
        .map(Assignment::from_repo)
        .collect::<Result<Vec<_>, _>>()?;
    page.page(rows, |a| a.id).map(Json)
}

/// Assign a course, or a module or item of it, to a user. Needs `enrollment.view` at the course
/// too, since the answer names the user. A user who is not a learner of the course is enrolled
/// as Student, which needs `enrollment.manage` at the course and the user within your
/// `user.view`.
#[utoipa::path(
    post,
    path = "/api/v1/courses/{course_id}/assignments",
    operation_id = "createAssignment",
    tag = "assignments",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("course_id" = String, Path, format = "uuid", description = "The course's id.")),
    request_body = NewAssignmentBody,
    responses(
        (
            status = 201,
            description = "The assignment is created",
            body = Assignment,
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
pub async fn create_assignment(
    require: Require<AssignmentManage, CourseTarget>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<NewAssignmentBody>,
) -> Result<Response, ApiError> {
    let (grants, course_id) = (require.grants(), require.target().id);
    let new = new_assignment(&body, course_id, require.principal().user_id)?;
    let mut tx = begin(&state).await?;
    let course = recheck_manage(&mut tx, grants, course_id, COURSE_NOT_FOUND).await?;
    refuse_without_roster(grants, &course)?;
    let scope = grants.scope();
    let learner = repo::is_learner(&mut tx, scope, course_id, new.user_id)
        .await
        .map_err(assignment_error)?;
    let enroll = if learner {
        Enroll::Refuse
    } else {
        refuse_enrolling(&mut tx, grants, &course, new.user_id).await?;
        Enroll::AsStudent(client.actor(require.principal()))
    };
    let now = state.clock.now();
    let created = repo::create(&mut *tx, scope, course_id, &new, enroll, now)
        .await
        .map_err(assignment_error)?;
    commit(tx).await?;
    let location = format!("{ASSIGNMENT_PATH}/{}", created.id);
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(Assignment::from_repo(created)?),
    )
        .into_response())
}

/// Read an assignment with its people's statuses.
#[utoipa::path(
    get,
    path = "/api/v1/assignments/{assignment_id}",
    operation_id = "getAssignment",
    tag = "assignments",
    params(("assignment_id" = String, Path, format = "uuid", description = "The assignment's id.")),
    responses(
        (status = 200, description = "The assignment", body = Assignment),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = AssignmentNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_assignment(
    require: Require<AssignmentManage, AssignmentTarget>,
    State(state): State<AppState>,
) -> Result<Json<Assignment>, ApiError> {
    let scope = require.grants().scope();
    let mut conn = acquire(&state).await?;
    let found = repo::get(&mut conn, scope, require.target().id, state.clock.now())
        .await
        .map_err(assignment_error)?
        .ok_or_else(assignment_not_found)?;
    Ok(Json(Assignment::from_repo(found)?))
}

/// Change an assignment's due rule. Every person who has not completed it gets their due date
/// again from the new rule; completed ones keep theirs.
#[utoipa::path(
    patch,
    path = "/api/v1/assignments/{assignment_id}",
    operation_id = "updateAssignment",
    tag = "assignments",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("assignment_id" = String, Path, format = "uuid", description = "The assignment's id.")),
    request_body = AssignmentPatch,
    responses(
        (status = 200, description = "The assignment after the change", body = Assignment),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = AssignmentWriteForbidden),
        (status = 404, response = AssignmentNotFound),
        (status = 409, response = AssignmentUpdateConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_assignment(
    require: Require<AssignmentManage, AssignmentTarget>,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<AssignmentPatch>,
) -> Result<Json<Assignment>, ApiError> {
    let grants = require.grants();
    let target = require.target();
    let due = body
        .due
        .map(|due| due.as_ref().map(AssignmentDue::rule).transpose())
        .transpose()?;
    let mut tx = begin(&state).await?;
    recheck_manage(&mut tx, grants, target.course.id, ASSIGNMENT_NOT_FOUND).await?;
    let now = state.clock.now();
    let changed = match due {
        Some(due) => repo::set_due(&mut *tx, grants.scope(), target.id, due, now).await,
        None => repo::get(&mut tx, grants.scope(), target.id, now)
            .await
            .and_then(|found| found.ok_or(repo::Error::NotFound)),
    }
    .map_err(assignment_error)?;
    commit(tx).await?;
    Ok(Json(Assignment::from_repo(changed)?))
}

/// End an assignment: it stops counting and its people's rows are kept. Ending an ended
/// assignment changes nothing.
#[utoipa::path(
    post,
    path = "/api/v1/assignments/{assignment_id}/end",
    operation_id = "endAssignment",
    tag = "assignments",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("assignment_id" = String, Path, format = "uuid", description = "The assignment's id.")),
    responses(
        (status = 200, description = "The ended assignment", body = Assignment),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = AssignmentWriteForbidden),
        (status = 404, response = AssignmentNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn end_assignment(
    require: Require<AssignmentManage, AssignmentTarget>,
    State(state): State<AppState>,
) -> Result<Json<Assignment>, ApiError> {
    let grants = require.grants();
    let target = require.target();
    let mut tx = begin(&state).await?;
    recheck_manage(&mut tx, grants, target.course.id, ASSIGNMENT_NOT_FOUND).await?;
    let ended = repo::end(&mut *tx, grants.scope(), target.id, state.clock.now())
        .await
        .map_err(assignment_error)?;
    commit(tx).await?;
    Ok(Json(Assignment::from_repo(ended)?))
}

/// The repository's form of `body`, or the 422 that refuses it.
fn new_assignment(
    body: &NewAssignmentBody,
    course_id: Uuid,
    created_by: Uuid,
) -> Result<NewAssignment, ApiError> {
    if body.assignee.assignee_type != AssigneeType::User {
        return Err(ApiError::invalid(vec![FieldError::at(
            &["assignee", "type"],
            code::ASSIGNMENT_ASSIGNEE_UNAVAILABLE,
        )]));
    }
    let target = body.target;
    if target.target_type == AssignmentTargetType::Course && target.id != course_id {
        return Err(ApiError::invalid(vec![FieldError::at(
            &["target", "id"],
            code::ASSIGNMENT_TARGET_MISMATCH,
        )]));
    }
    Ok(NewAssignment {
        target_type: target.target_type.into(),
        target_id: target.id,
        user_id: body.assignee.id,
        due: body.due.as_ref().map(AssignmentDue::rule).transpose()?,
        created_by,
    })
}

/// 403 `permission.denied` unless the caller holds `enrollment.view` at the course. An
/// assignment names its assignee, so listing a course's assignments, or assigning someone and
/// learning from the answer whether they are a learner of the course, would show its roster to
/// a holder of `assignment.manage` alone.
fn refuse_without_roster(grants: &Grants, course: &Target) -> Result<(), ApiError> {
    if grants.holds::<EnrollmentView>(course) {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::FORBIDDEN,
        PERMISSION_DENIED,
        "An assignment names its assignee, so it needs `enrollment.view` at this course as well as `assignment.manage`.",
    ))
}

/// B3's checks for enrolling the user `user_id` as Student through an assignment, the ones
/// `POST /api/v1/courses/{course_id}/enrollments` makes for that role: 404 `user.not_found`
/// unless the caller's `user.view` covers the user, 403 `permission.denied` without
/// `enrollment.manage` at the course, 403 `permission.escalation` without every Student
/// permission grantable at organization or account scope held there. The reach check reads
/// through `tx`, the transaction holding the course lock, so the request never waits on a
/// second pool connection while it holds the lock.
async fn refuse_enrolling(
    tx: &mut Transaction<'static, Postgres>,
    grants: &Grants,
    course: &Target,
    user_id: Uuid,
) -> Result<(), ApiError> {
    refuse_unreachable_user(&mut **tx, grants, user_id).await?;
    if !grants.holds::<EnrollmentManage>(course) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            PERMISSION_DENIED,
            "The user is not a learner of this course, so assigning them enrolls them as Student, which needs `enrollment.manage` here.",
        ));
    }
    let student = roles::builtin(repo::STUDENT_ROLE)
        .ok_or_else(|| ApiError::internal(&repo::Error::StudentRoleMissing))?
        .permissions()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    refuse_escalation(grants, &student, course)
}

/// The extractor's `assignment.manage` check again ([`check_course`]), on the course as
/// `courses::lock` reads it in the transaction, which locks the course row `FOR NO KEY UPDATE`
/// and its account `FOR SHARE`: 404 `not_found` when the course is gone, out of the caller's
/// reach, or a draft or template without `course.view_all`; 403 when it is visible without
/// `assignment.manage`. Returns the course as the evaluator's target.
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
    check_course::<AssignmentManage>(grants, &course, needs_view_all, not_found)?;
    Ok(course.target())
}

async fn acquire(state: &AppState) -> Result<PoolConnection<Postgres>, ApiError> {
    state
        .db
        .acquire()
        .await
        .map_err(|error| ApiError::internal(&error))
}

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

fn assignment_not_found() -> ApiError {
    not_found_as(Denied::CourseNotFound, ASSIGNMENT_NOT_FOUND)
}

/// The answer for an assignments repository error.
fn assignment_error(error: repo::Error) -> ApiError {
    use repo::Error as E;
    use stepclimb_core::enrollments::Error as EnrollmentError;
    let conflict = |code, detail| ApiError::new(StatusCode::CONFLICT, code, detail);
    let not_found = |code| not_found_as(Denied::CourseNotFound, code);
    let archived = || conflict(COURSE_ARCHIVED, "An archived course takes no assignment.");
    let template = || {
        conflict(
            COURSE_TEMPLATE,
            "A template takes no assignment. Copy it to an ordinary course first.",
        )
    };
    match error {
        E::NotFound => assignment_not_found(),
        E::CourseNotFound => not_found(COURSE_NOT_FOUND),
        E::TargetNotFound(TargetType::Course) => ApiError::invalid(vec![FieldError::at(
            &["target", "id"],
            code::ASSIGNMENT_TARGET_MISMATCH,
        )]),
        E::TargetNotFound(TargetType::Module) => not_found(MODULE_NOT_FOUND),
        E::TargetNotFound(TargetType::Item) => not_found(ITEM_NOT_FOUND),
        E::UserNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            USER_NOT_FOUND,
            "No user with this id exists in this organization.",
        ),
        E::CourseArchived | E::Enrollment(EnrollmentError::CourseArchived) => archived(),
        E::CourseTemplate | E::Enrollment(EnrollmentError::Template) => template(),
        E::EnrollmentExternallyManaged => conflict(
            code::ENROLLMENT_EXTERNALLY_MANAGED,
            "An outside system owns the user's Student enrollment in this course. Reactivate it there.",
        ),
        E::Ended => conflict(
            code::ASSIGNMENT_ENDED,
            "The assignment has ended and takes no change.",
        ),
        E::Due(DueError::DaysOutOfRange) => ApiError::invalid(vec![FieldError::at(
            &["due", "relative_days"],
            code::ASSIGNMENT_DUE_INVALID,
        )]),
        E::Due(DueError::Time(_)) => {
            ApiError::invalid(vec![FieldError::at(&["due"], code::ASSIGNMENT_DUE_INVALID)])
        }
        other @ (E::EnrollmentNeeded
        | E::StudentRoleMissing
        | E::UnknownTimeZone(_)
        | E::UnknownStoredValue { .. }
        | E::Enrollment(_)
        | E::Course(_)
        | E::Organization(_)
        | E::Role(_)
        | E::Time(_)
        | E::Database(_)) => ApiError::internal(&other),
    }
}

/// The subscriber of `item.completed` (spec 05 B10, spec 13 B8).
pub const ON_ITEM_COMPLETED: Kind = Kind::new("assignments.on_item_completed", "default");

/// The subscriber of `module.completed` (spec 05 B10, spec 13 B8).
pub const ON_MODULE_COMPLETED: Kind = Kind::new("assignments.on_module_completed", "default");

/// The subscriber of `course.completed` (spec 05 B10, spec 13 B8).
pub const ON_COURSE_COMPLETED: Kind = Kind::new("assignments.on_course_completed", "default");

/// Subscribe the three completion handlers to the progress topics, which must be registered
/// first (`progress::register_topics`). A registry lets a kind subscribe to one topic, so B8's
/// one subscriber is a kind per topic.
pub fn register_completion(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.subscribe(&ITEM_COMPLETED, ON_ITEM_COMPLETED, |ctx| {
        complete(ctx, TargetType::Item)
    })?;
    registry.subscribe(&MODULE_COMPLETED, ON_MODULE_COMPLETED, |ctx| {
        complete(ctx, TargetType::Module)
    })?;
    registry.subscribe(&COURSE_COMPLETED, ON_COURSE_COMPLETED, |ctx| {
        complete(ctx, TargetType::Course)
    })
}

/// The `data` of a completion event, as `progress::emit` writes it.
#[derive(Deserialize)]
struct CompletionData {
    user_id: Uuid,
    course_id: Uuid,
    module_id: Option<Uuid>,
    item_id: Option<Uuid>,
    completed_at: String,
}

/// Complete the rows the event names (B8). Running it twice completes nothing more.
async fn complete(ctx: JobContext, target_type: TargetType) -> Outcome {
    let event = match outbox::event(&ctx).await {
        Ok(event) => event,
        Err(outcome) => return outcome,
    };
    let Some(scope) = ctx.scope() else {
        return Outcome::fail("an event job runs for an organization");
    };
    let Some(completed) = completion(&event.payload, target_type) else {
        return Outcome::fail("the event's data is not a completion of this topic");
    };
    match repo::record_completion(ctx.pool(), scope, &completed, ctx.clock().now()).await {
        Ok(_) => Outcome::Done,
        Err(error) => Outcome::retry(format_args!("could not record the completion: {error}")),
    }
}

/// The completion an event's envelope reports, for its topic's target type.
fn completion(payload: &serde_json::Value, target_type: TargetType) -> Option<Completed> {
    let data = CompletionData::deserialize(payload.get("data")?).ok()?;
    let target_id = match target_type {
        TargetType::Course => data.course_id,
        TargetType::Module => data.module_id?,
        TargetType::Item => data.item_id?,
    };
    Some(Completed {
        user_id: data.user_id,
        target_type,
        target_id,
        completed_at: courses::parse_timestamp(&data.completed_at)?,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn due(value: serde_json::Value) -> Result<DueRule, ApiError> {
        serde_json::from_value::<AssignmentDue>(value)
            .expect("a due rule's shape")
            .rule()
    }

    #[test]
    fn a_due_rule_names_exactly_one_form_within_its_range() {
        assert_eq!(
            due(json!({ "relative_days": 14 })).unwrap(),
            DueRule::RelativeDays(14)
        );
        assert!(matches!(
            due(json!({ "fixed": "2026-03-15T12:00:00-05:00" })).unwrap(),
            DueRule::Fixed(_)
        ));
        for (value, pointer) in [
            (json!({}), "/due"),
            (
                json!({ "fixed": "2026-03-15T12:00:00Z", "relative_days": 3 }),
                "/due",
            ),
            (json!({ "relative_days": 0 }), "/due/relative_days"),
            (json!({ "relative_days": 3651 }), "/due/relative_days"),
            (json!({ "relative_days": 70000 }), "/due/relative_days"),
            (json!({ "fixed": "2026-03-15" }), "/due/fixed"),
            (json!({ "fixed": "1969-12-31T23:59:59Z" }), "/due/fixed"),
            (json!({ "fixed": "2200-01-01T00:00:00Z" }), "/due/fixed"),
        ] {
            let refused = due(value.clone()).unwrap_err();
            assert_eq!(refused.code(), "request.invalid", "{value}");
            let problem = serde_json::to_value(refused.problem()).unwrap();
            assert_eq!(problem["errors"][0]["field"], pointer, "{value}");
            assert_eq!(
                problem["errors"][0]["code"], "assignment.due_invalid",
                "{value}"
            );
        }
        assert!(due(json!({ "fixed": "2199-12-31T23:59:59Z" })).is_ok());
    }

    #[test]
    fn a_completion_names_its_topic_target() {
        let (user, course, module, item) = (
            Uuid::now_v7(),
            Uuid::now_v7(),
            Uuid::now_v7(),
            Uuid::now_v7(),
        );
        let envelope = json!({
            "type": "item.completed",
            "timestamp": "2026-03-01T10:00:00Z",
            "data": {
                "user_id": user, "course_id": course, "module_id": module, "item_id": item,
                "item_kind": "page", "requirement_kind": "view",
                "completed_at": "2026-03-01T10:00:00.000000Z",
            },
        });
        for (target_type, id) in [
            (TargetType::Item, item),
            (TargetType::Module, module),
            (TargetType::Course, course),
        ] {
            let completed = completion(&envelope, target_type).unwrap();
            assert_eq!(completed.target_id, id);
            assert_eq!(completed.user_id, user);
        }
        let course_only = json!({ "data": {
            "user_id": user, "course_id": course, "completed_at": "2026-03-01T10:00:00Z",
        }});
        assert!(completion(&course_only, TargetType::Item).is_none());
        assert!(completion(&json!({}), TargetType::Course).is_none());
    }
}
