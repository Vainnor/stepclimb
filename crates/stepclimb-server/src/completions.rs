// SPDX-License-Identifier: AGPL-3.0-or-later
//! Completion history over the API (docs/specs/m1/14-recurrence-and-expiring-completions.md
//! B13, API).
//!
//! `GET /api/v1/courses/{course_id}/completions` lists the course's and its modules' completions,
//! newest first. It takes `Require<perm::CourseView, CourseTarget>` and then decides in the
//! handler as the staff progress reads do (spec 05 B20, `progress::reach`): `progress.view` at the
//! course reads every learner's history, `progress.view_observed` only the learners the caller's
//! active Observer enrollments name, and anyone else gets 403 `permission.denied`. Without
//! `course.view_all` at the course, completions of unpublished or deleted modules are left out.
//!
//! `GET /api/v1/me/completions` lists the caller's own history in their organization, across
//! courses, and needs no permission: the rows are the caller's. A title the caller could not
//! read now answers null ([`hide_titles`]): without `course.view_all` at the course, the title of
//! a draft or template course and of an unpublished or deleted module (spec 02 B10, spec 05
//! B14).

use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use stepclimb_core::{
    OrgScope,
    completions::{self, Completion as CoreCompletion, Error, TargetType},
    courses,
    grants::{Grants, Target},
    id::Uuid,
    perm::{CourseView, CourseViewAll},
    progress::Shown,
};
use utoipa::{IntoParams, ToSchema};

use crate::{
    AppState,
    error::ApiError,
    extract::ApiQuery,
    pagination::{Page, PageParams},
    progress::{
        LearnerProgressForbidden, LearnerProgressListNotFound, ProgressUser, acquire, reach,
    },
    require::{AuthOnly, CourseTarget, Require},
    timestamp,
};

/// What a completion is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompletionTargetType {
    Course,
    Module,
}

impl From<TargetType> for CompletionTargetType {
    fn from(target: TargetType) -> Self {
        match target {
            TargetType::Course => Self::Course,
            TargetType::Module => Self::Module,
        }
    }
}

impl From<CompletionTargetType> for TargetType {
    fn from(target: CompletionTargetType) -> Self {
        match target {
            CompletionTargetType::Course => Self::Course,
            CompletionTargetType::Module => Self::Module,
        }
    }
}

/// The course or module a completion is of.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct CompletionTarget {
    #[serde(rename = "type")]
    pub target_type: CompletionTargetType,
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    /// The course's or module's title now; null for a module deleted since, and on your own
    /// history where you cannot read it now without `course.view_all` (a draft or template
    /// course, an unpublished module).
    #[schema(required = true)]
    pub title: Option<String>,
}

/// One completion of a course or a module, in one cycle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Completion {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub user: ProgressUser,
    pub target: CompletionTarget,
    /// 1 for the user's first completion of the target, then 2, and so on.
    pub cycle: i32,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub completed_at: String,
    /// RFC 3339, UTC: when the completion stops being valid; null when it does not expire.
    #[schema(format = DateTime, required = true)]
    pub expires_at: Option<String>,
    /// RFC 3339, UTC: when the completion expired; null while it is valid.
    #[schema(format = DateTime, required = true)]
    pub expired_at: Option<String>,
}

impl Completion {
    fn from_core(completion: CoreCompletion) -> Result<Self, ApiError> {
        let time = |t| timestamp::rfc3339(t).map_err(|error| ApiError::internal(&error));
        Ok(Self {
            id: completion.id,
            user: ProgressUser {
                id: completion.user_id,
                display_name: completion.display_name,
            },
            target: CompletionTarget {
                target_type: completion.target_type.into(),
                id: completion.target_id,
                title: completion.target_title,
            },
            cycle: completion.cycle,
            completed_at: time(completion.completed_at)?,
            expires_at: completion.expires_at.map(time).transpose()?,
            expired_at: completion.expired_at.map(time).transpose()?,
        })
    }

    /// The sort key of the row in its list.
    fn key(row: &CoreCompletion) -> Result<completions::ListKey, ApiError> {
        let at = stepclimb_core::db::epoch_micros(row.completed_at)
            .map_err(|error| ApiError::internal(&error))?;
        Ok((at, row.id))
    }
}

/// The filters of a course's completion history.
#[derive(Clone, Copy, Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CourseCompletionParams {
    /// Only this user's completions.
    #[param(value_type = Option<String>, format = "uuid")]
    pub user_id: Option<Uuid>,
    /// Only completions of courses or only of modules.
    #[param(value_type = Option<CompletionTargetType>)]
    pub target_type: Option<CompletionTargetType>,
}

/// The filter of your completion history.
#[derive(Clone, Copy, Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct MyCompletionParams {
    /// Only completions of this course and its modules.
    #[param(value_type = Option<String>, format = "uuid")]
    pub course_id: Option<Uuid>,
}

/// List the completion history of a course and its modules, newest first: one row per
/// completion per cycle. With `progress.view` you read every learner's; with only
/// `progress.view_observed`, the learners your active Observer enrollments in the course name.
#[utoipa::path(
    get,
    path = "/api/v1/courses/{course_id}/completions",
    operation_id = "listCourseCompletions",
    tag = "progress",
    params(
        ("course_id" = String, Path, format = "uuid", description = "The course's id."),
        CourseCompletionParams,
        PageParams,
    ),
    responses(
        (status = 200, description = "One page of completions", body = Page<Completion>),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = LearnerProgressForbidden),
        (status = 404, response = LearnerProgressListNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_course_completions(
    require: Require<CourseView, CourseTarget>,
    State(state): State<AppState>,
    ApiQuery(filter): ApiQuery<CourseCompletionParams>,
    ApiQuery(params): ApiQuery<PageParams>,
) -> Result<Json<Page<Completion>>, ApiError> {
    let page = params.parse::<completions::ListKey>()?;
    let mut conn = acquire(&state).await?;
    let reach = reach(&mut conn, &require).await?;
    let filter = completions::CourseFilter {
        user_id: filter.user_id,
        target_type: filter.target_type.map(Into::into),
        only: reach.only,
        published_only: reach.shown == Shown::Published,
    };
    let (scope, course_id) = (require.grants().scope(), require.target().id);
    let rows = completions::list_for_course(
        &mut *conn,
        scope,
        course_id,
        &filter,
        page.after(),
        page.fetch_limit(),
    )
    .await
    .map_err(completion_error)?;
    answer(&page, rows)
}

/// List your own completion history, newest first: one row per course or module completion per
/// cycle, across your courses. Without `course.view_all` at a course, the title of a draft or
/// template course and of an unpublished or deleted module is null.
#[utoipa::path(
    get,
    path = "/api/v1/me/completions",
    operation_id = "listMyCompletions",
    tag = "me",
    params(MyCompletionParams, PageParams),
    responses(
        (status = 200, description = "One page of your completions", body = Page<Completion>),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_my_completions(
    auth: AuthOnly,
    State(state): State<AppState>,
    ApiQuery(filter): ApiQuery<MyCompletionParams>,
    ApiQuery(params): ApiQuery<PageParams>,
) -> Result<Json<Page<Completion>>, ApiError> {
    let page = params.parse::<completions::ListKey>()?;
    let principal = auth.principal();
    let rows = completions::list_for_user(
        &state.db,
        principal.scope(),
        principal.user_id,
        filter.course_id,
        page.after(),
        page.fetch_limit(),
    )
    .await
    .map_err(completion_error)?;
    let rows = hide_titles(&state, auth.grants(), principal.scope(), rows).await?;
    answer(&page, rows)
}

/// `rows` with `target_title` cleared where the caller may not read it: the row's title is
/// hidden from learners and `grants` hold no `course.view_all` at its course.
async fn hide_titles(
    state: &AppState,
    grants: &Grants,
    scope: OrgScope,
    mut rows: Vec<CoreCompletion>,
) -> Result<Vec<CoreCompletion>, ApiError> {
    let mut view_all: HashMap<Uuid, bool> = HashMap::new();
    for row in rows.iter_mut().filter(|row| row.title_hidden) {
        let holds = match view_all.get(&row.course_id) {
            Some(holds) => *holds,
            None => {
                let holds = holds_view_all(state, grants, scope, row.course_id).await?;
                view_all.insert(row.course_id, holds);
                holds
            }
        };
        if !holds {
            row.target_title = None;
        }
    }
    Ok(rows)
}

/// Whether `grants` hold `course.view_all` at the course `course_id`, through its account path.
async fn holds_view_all(
    state: &AppState,
    grants: &Grants,
    scope: OrgScope,
    course_id: Uuid,
) -> Result<bool, ApiError> {
    let path = courses::path(&state.db, scope, course_id)
        .await
        .map_err(|error| ApiError::internal(&error))?;
    Ok(path.is_some_and(|account_path| {
        grants.holds::<CourseViewAll>(&Target::Course {
            account_path,
            course_id,
        })
    }))
}

/// The page of `rows`, which the repository fetched after the request's cursor.
fn answer(
    page: &crate::pagination::PageRequest<completions::ListKey>,
    rows: Vec<CoreCompletion>,
) -> Result<Json<Page<Completion>>, ApiError> {
    let keyed = rows
        .into_iter()
        .map(|row| Ok((Completion::key(&row)?, Completion::from_core(row)?)))
        .collect::<Result<Vec<_>, ApiError>>()?;
    let page = page.page(keyed, |(key, _)| *key)?;
    Ok(Json(Page {
        items: page.items.into_iter().map(|(_, row)| row).collect(),
        next_cursor: page.next_cursor,
    }))
}

fn completion_error(error: Error) -> ApiError {
    match error {
        Error::InvalidCursor => ApiError::invalid_cursor(),
        other => ApiError::internal(&other),
    }
}
