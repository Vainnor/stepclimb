// SPDX-License-Identifier: AGPL-3.0-or-later
//! Learner progress over the API and its events (docs/specs/m1/05-progress-and-events.md B1 to
//! B3, B10 to B14, B18, B20, API).
//!
//! `POST /api/v1/items/{item_id}/progress` records a learner's step through
//! `stepclimb_core::progress::record_step`. It takes `Require<perm::CourseParticipate,
//! ItemTarget>`: the target passes `require::check_course` (a course the caller cannot see, a
//! draft or a template answers 404 `item.not_found`), and the handler runs it again on the course
//! row its transaction holds `FOR SHARE` (`modules::recheck_shared`), before the repository checks the lock (`item.locked`,
//! spec 04 B13).
//!
//! `GET /api/v1/courses/{course_id}/progress` and `.../progress/{user_id}` are the staff reads
//! (B14). They take `Require<perm::CourseView, CourseTarget>` and then decide in the handler
//! (B20): `progress.view` at the course reads every learner, `progress.view_observed` (only an
//! active enrollment gives it) only the learners the caller's active Observer enrollments name,
//! and anyone else gets 403 `permission.denied`. The list filters to the observed learners; the
//! detail answers 404 `progress.not_found` for anyone else. Without `course.view_all` at the
//! course the answer leaves out unpublished modules and items.
//!
//! The events of what became complete are written to the outbox by [`emit`], in the transaction
//! that completed it (B10). `stepclimb_core::progress` returns them, since that crate cannot
//! reach the outbox; every caller of `record_step` or `complete_item` passes them to [`emit`]
//! before it commits. [`register_topics`] gives the job registry the three topics, so the
//! dispatcher claims their events. The event bodies are the schemas below, which the OpenAPI
//! document lists as components.

use std::time::SystemTime;

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, Postgres, Transaction, pool::PoolConnection};
use stepclimb_core::{
    OrgScope, enrollments,
    grants::Denied,
    id::Uuid,
    perm::{CourseParticipate, CourseView, CourseViewAll, ProgressView, ProgressViewObserved},
    progress::{self, Error, Event},
    rules,
};
use stepclimb_jobs::{Registry, RegistryError, outbox::Topic};
use utoipa::{IntoParams, ToSchema};

use crate::{
    AppState,
    course_enrollments::EnrollmentStatus,
    error::{ApiError, FieldError},
    extract::{ApiJson, ApiPath, ApiQuery},
    modules::{ItemKind, begin, code::COURSE_ARCHIVED, commit, content_error, recheck_shared},
    openapi::problem_responses,
    pagination::{Page, PageParams},
    require::{
        CourseTarget, ItemTarget, Require,
        code::{ITEM_NOT_FOUND, PERMISSION_DENIED},
    },
    timestamp, unlock,
};

/// The codes this module produces besides the target's and the lock's.
pub mod code {
    /// The item does not take the step (a 422 field code at `/action`).
    pub const PROGRESS_ACTION_INVALID: &str = "progress.action_invalid";
    /// The user is not a learner of the course, or the caller does not observe them (404).
    pub const PROGRESS_NOT_FOUND: &str = "progress.not_found";
}

/// `item.completed`, schema version 1 (B11).
pub const ITEM_COMPLETED: Topic = Topic::new("item.completed", 1);
/// `module.completed`, schema version 1 (B11).
pub const MODULE_COMPLETED: Topic = Topic::new("module.completed", 1);
/// `course.completed`, schema version 1 (B11).
pub const COURSE_COMPLETED: Topic = Topic::new("course.completed", 1);

/// Register the three progress topics, so the outbox dispatcher claims their events.
pub fn register_topics(registry: &mut Registry) -> Result<(), RegistryError> {
    for topic in [ITEM_COMPLETED, MODULE_COMPLETED, COURSE_COMPLETED] {
        registry.register_topic(topic)?;
    }
    Ok(())
}

problem_responses! {
    ProgressNotFound => "`item.not_found`: no published item with this id in a published module exists in the organization, or you cannot see its course; a draft course or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined.",
    ProgressForbidden => "`permission.denied`: you are not a learner of the item's course (an active enrollment whose role holds `course.participate`). `item.locked`: the item is locked for you; `reasons` says what it waits for. `auth.csrf_refused`: the request did not come from this site.",
    ProgressConflict => "`course.archived`: the course is archived and takes no progress.",
    ProgressInvalid => "`request.invalid`: `errors` names `/action` with `progress.action_invalid`: `view` on an item that is not a page, a file or an external link, or `mark_done` on an item whose requirement is not `mark_done`.",
}

/// A step a learner reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProgressAction {
    /// You opened a page, a file or an external link.
    View,
    /// You marked an item whose requirement is `mark_done` done.
    MarkDone,
}

impl From<ProgressAction> for progress::Action {
    fn from(action: ProgressAction) -> Self {
        match action {
            ProgressAction::View => Self::View,
            ProgressAction::MarkDone => Self::MarkDone,
        }
    }
}

/// The body of a progress step.
#[derive(Clone, Copy, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProgressStep {
    pub action: ProgressAction,
}

/// Where a learner is with an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProgressState {
    Started,
    Completed,
}

/// Your progress on one item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ItemProgress {
    #[schema(value_type = String, format = "uuid")]
    pub item_id: Uuid,
    pub state: ProgressState,
    /// RFC 3339, UTC: your first step on the item.
    #[schema(format = DateTime)]
    pub started_at: String,
    /// RFC 3339, UTC: when you completed it; null until then.
    #[schema(format = DateTime, required = true)]
    pub completed_at: Option<String>,
}

impl ItemProgress {
    pub(crate) fn from_core(progress: progress::ItemProgress) -> Result<Self, ApiError> {
        Ok(Self {
            item_id: progress.item_id,
            state: match progress.state {
                progress::State::Started => ProgressState::Started,
                progress::State::Completed => ProgressState::Completed,
            },
            started_at: time(progress.started_at)?,
            completed_at: progress.completed_at.map(time).transpose()?,
        })
    }
}

/// Your roll-up for a course: its modules that have a published requirement, and how many of
/// them you completed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct CourseProgress {
    pub required_count: i32,
    pub completed_count: i32,
    /// RFC 3339, UTC: when you completed the course; null until then. Never cleared.
    #[schema(format = DateTime, required = true)]
    pub completed_at: Option<String>,
}

impl CourseProgress {
    pub(crate) fn from_core(progress: progress::CourseProgress) -> Result<Self, ApiError> {
        Ok(Self {
            required_count: progress.required_count,
            completed_count: progress.completed_count,
            completed_at: progress.completed_at.map(time).transpose()?,
        })
    }
}

fn time(t: SystemTime) -> Result<String, ApiError> {
    timestamp::rfc3339(t).map_err(|error| ApiError::internal(&error))
}

/// Record a step of yours on an item: `view` for a page, a file or an external link, or
/// `mark_done` for an item whose requirement is `mark_done`. A step that meets the item's
/// requirement completes it, and completing an item can complete its module and the course. A
/// step that changes nothing answers your progress as it is. Completion is final.
#[utoipa::path(
    post,
    path = "/api/v1/items/{item_id}/progress",
    operation_id = "recordItemProgress",
    tag = "progress",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("item_id" = String, Path, format = "uuid", description = "The item's id.")),
    request_body = ProgressStep,
    responses(
        (status = 200, description = "Your progress on the item after the step", body = ItemProgress),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ProgressForbidden),
        (status = 404, response = ProgressNotFound),
        (status = 409, response = ProgressConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = ProgressInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn record_item_progress(
    require: Require<CourseParticipate, ItemTarget>,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<ProgressStep>,
) -> Result<Json<ItemProgress>, ApiError> {
    let grants = require.grants();
    let scope = grants.scope();
    let (user_id, item_id) = (require.principal().user_id, require.target().id);
    let now = state.clock.now();
    let mut tx = begin(&state).await?;
    // `require`'s course check again, on the course row this transaction holds `FOR SHARE`: a
    // course made a draft, or out of reach, after the extractor ran answers 404. `FOR SHARE`, so
    // learners' steps in one course do not wait for each other.
    recheck_shared(
        &mut tx,
        &require,
        require.target().course.id,
        ITEM_NOT_FOUND,
    )
    .await?;
    let recorded = progress::record_step(&mut tx, scope, user_id, item_id, body.action.into(), now)
        .await
        .map_err(progress_error)?;
    emit(&mut tx, scope, &recorded.events, now).await?;
    commit(tx).await?;
    Ok(Json(ItemProgress::from_core(recorded.progress)?))
}

/// A learner as a progress list names them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ProgressUser {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub display_name: String,
}

/// A learner's roll-up for one module: its published items with a requirement, and how many of
/// them the learner completed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ModuleProgress {
    #[schema(value_type = String, format = "uuid")]
    pub module_id: Uuid,
    pub required_count: i32,
    pub completed_count: i32,
    /// RFC 3339, UTC: when the learner completed the module; null until then. Never cleared.
    #[schema(format = DateTime, required = true)]
    pub completed_at: Option<String>,
}

impl ModuleProgress {
    fn from_core(progress: progress::ModuleProgress) -> Result<Self, ApiError> {
        Ok(Self {
            module_id: progress.module_id,
            required_count: progress.required_count,
            completed_count: progress.completed_count,
            completed_at: progress.completed_at.map(time).transpose()?,
        })
    }
}

/// One learner of a course and their progress. `modules` lists the modules the learner
/// completed an item of, in course order; without `course.view_all` at the course, published
/// modules only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct LearnerProgress {
    pub user: ProgressUser,
    /// `active` when any of the learner's enrollments in the course that make them a learner
    /// is, otherwise `concluded`.
    pub enrollment_status: EnrollmentStatus,
    /// Null until the learner first completes an item of the course.
    #[schema(required = true)]
    pub course: Option<CourseProgress>,
    pub modules: Vec<ModuleProgress>,
}

impl LearnerProgress {
    fn from_core(learner: progress::Learner) -> Result<Self, ApiError> {
        Ok(Self {
            user: ProgressUser {
                id: learner.user_id,
                display_name: learner.display_name,
            },
            enrollment_status: learner.enrollment_status.into(),
            course: learner.course.map(CourseProgress::from_core).transpose()?,
            modules: learner
                .modules
                .into_iter()
                .map(ModuleProgress::from_core)
                .collect::<Result<_, _>>()?,
        })
    }
}

/// One learner of a course with their progress on each item they took a step on, in course
/// order; without `course.view_all` at the course, published items in published modules only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct LearnerProgressDetail {
    pub user: ProgressUser,
    pub enrollment_status: EnrollmentStatus,
    #[schema(required = true)]
    pub course: Option<CourseProgress>,
    pub modules: Vec<ModuleProgress>,
    pub items: Vec<ItemProgress>,
}

/// The filter of a course's progress list.
#[derive(Clone, Copy, Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LearnerProgressParams {
    /// Only learners whose enrollment status is this.
    #[param(value_type = Option<EnrollmentStatus>)]
    pub status: Option<EnrollmentStatus>,
}

/// The path of one learner's progress in a course.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct LearnerPath {
    pub course_id: Uuid,
    pub user_id: Uuid,
}

problem_responses! {
    LearnerProgressForbidden => "`permission.denied`: you do not hold `progress.view` at the course, nor `progress.view_observed` through an active enrollment in it.",
    LearnerProgressListNotFound => "`course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined.",
    LearnerProgressNotFound => "`course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `progress.not_found`: the user is not a learner of the course (no enrollment there, active or concluded, whose role holds `course.participate`), or you read only the learners you observe and do not observe them. `org.not_found`: the request's organization cannot be determined.",
}

/// Whose progress the caller reads in the course, and which content (B20).
struct Reach {
    /// `None`: every learner. `Some`: only these, the learners the caller observes.
    only: Option<Vec<Uuid>>,
    shown: progress::Shown,
}

/// B20: `progress.view` at the course reads every learner; otherwise `progress.view_observed`,
/// which only an active enrollment gives, reads the learners the caller's active Observer
/// enrollments in the course name; anyone else gets 403 `permission.denied`. Content the caller
/// cannot read without `course.view_all` stays out of the answer.
async fn reach(
    conn: &mut PgConnection,
    require: &Require<CourseView, CourseTarget>,
) -> Result<Reach, ApiError> {
    let (grants, course) = (require.grants(), require.target());
    let target = course.target();
    let shown = if grants.holds::<CourseViewAll>(&target) {
        progress::Shown::All
    } else {
        progress::Shown::Published
    };
    if grants.holds::<ProgressView>(&target) {
        return Ok(Reach { only: None, shown });
    }
    if grants.holds::<ProgressViewObserved>(&target) {
        let observer = require.principal().user_id;
        let only = enrollments::observed_by(conn, grants.scope(), course.id, observer)
            .await
            .map_err(|error| ApiError::internal(&error))?;
        return Ok(Reach {
            only: Some(only),
            shown,
        });
    }
    Err(Denied::Forbidden.into())
}

/// List the learners of a course with their course and module progress, sorted by display
/// name, then user id. With `progress.view` you read every learner; with only
/// `progress.view_observed`, the learners your active Observer enrollments in the course name.
#[utoipa::path(
    get,
    path = "/api/v1/courses/{course_id}/progress",
    operation_id = "listCourseProgress",
    tag = "progress",
    params(
        ("course_id" = String, Path, format = "uuid", description = "The course's id."),
        LearnerProgressParams,
        PageParams,
    ),
    responses(
        (status = 200, description = "One page of learners", body = Page<LearnerProgress>),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = LearnerProgressForbidden),
        (status = 404, response = LearnerProgressListNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_course_progress(
    require: Require<CourseView, CourseTarget>,
    State(state): State<AppState>,
    ApiQuery(filter): ApiQuery<LearnerProgressParams>,
    ApiQuery(params): ApiQuery<PageParams>,
) -> Result<Json<Page<LearnerProgress>>, ApiError> {
    let page = params.parse::<progress::LearnerKey>()?;
    let mut conn = acquire(&state).await?;
    let reach = reach(&mut conn, &require).await?;
    let filter = progress::LearnerFilter {
        status: filter.status.map(Into::into),
        only: reach.only,
    };
    let (scope, course_id) = (require.grants().scope(), require.target().id);
    let (after, limit) = (page.after(), page.fetch_limit());
    let rows = progress::list_for_course(
        &mut conn,
        scope,
        course_id,
        &filter,
        reach.shown,
        after,
        limit,
    )
    .await
    .map_err(|error| match error {
        Error::InvalidCursor => ApiError::invalid_cursor(),
        other => ApiError::internal(&other),
    })?;
    let rows = rows
        .into_iter()
        .map(LearnerProgress::from_core)
        .collect::<Result<Vec<_>, _>>()?;
    page.page(rows, |l| (l.user.display_name.clone(), l.user.id))
        .map(Json)
}

/// Read one learner's progress in a course: the course, each module and each item they took a
/// step on. With only `progress.view_observed`, only a learner you observe.
#[utoipa::path(
    get,
    path = "/api/v1/courses/{course_id}/progress/{user_id}",
    operation_id = "getLearnerProgress",
    tag = "progress",
    params(
        ("course_id" = String, Path, format = "uuid", description = "The course's id."),
        ("user_id" = String, Path, format = "uuid", description = "The learner's user id."),
    ),
    responses(
        (status = 200, description = "The learner's progress", body = LearnerProgressDetail),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = LearnerProgressForbidden),
        (status = 404, response = LearnerProgressNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_learner_progress(
    require: Require<CourseView, CourseTarget>,
    State(state): State<AppState>,
    ApiPath(path): ApiPath<LearnerPath>,
) -> Result<Json<LearnerProgressDetail>, ApiError> {
    let mut conn = acquire(&state).await?;
    let reach = reach(&mut conn, &require).await?;
    if reach
        .only
        .as_ref()
        .is_some_and(|only| !only.contains(&path.user_id))
    {
        return Err(progress_not_found());
    }
    let (scope, course_id) = (require.grants().scope(), require.target().id);
    let detail =
        progress::learner_in_course(&mut conn, scope, course_id, path.user_id, reach.shown)
            .await
            .map_err(|error| ApiError::internal(&error))?
            .ok_or_else(progress_not_found)?;
    let learner = LearnerProgress::from_core(detail.learner)?;
    Ok(Json(LearnerProgressDetail {
        user: learner.user,
        enrollment_status: learner.enrollment_status,
        course: learner.course,
        modules: learner.modules,
        items: detail
            .items
            .into_iter()
            .map(ItemProgress::from_core)
            .collect::<Result<_, _>>()?,
    }))
}

fn progress_not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        code::PROGRESS_NOT_FOUND,
        "This user is not a learner of the course, or you cannot read their progress.",
    )
}

async fn acquire(state: &AppState) -> Result<PoolConnection<Postgres>, ApiError> {
    state
        .db
        .acquire()
        .await
        .map_err(|error| ApiError::internal(&error))
}

/// The answer for a progress repository error.
pub(crate) fn progress_error(error: Error) -> ApiError {
    match error {
        Error::ItemNotFound => unlock::not_found(),
        Error::Locked(reasons) => unlock::item_locked(&reasons),
        Error::CourseArchived => ApiError::new(
            StatusCode::CONFLICT,
            COURSE_ARCHIVED,
            "The course is archived and takes no progress.",
        ),
        Error::NotLearner => ApiError::new(
            StatusCode::FORBIDDEN,
            PERMISSION_DENIED,
            "You are not a learner of this course.",
        ),
        Error::ActionInvalid => ApiError::invalid(vec![FieldError::at(
            &["action"],
            code::PROGRESS_ACTION_INVALID,
        )]),
        Error::Content(error) => content_error(error),
        Error::Course(error) => ApiError::internal(&error),
        other @ (Error::CompletionMismatch
        | Error::InvalidCursor
        | Error::Stored { .. }
        | Error::Time(_)
        | Error::Database(_)) => ApiError::internal(&other),
    }
}

/// The `data` of `item.completed` (B11): ids and facts, never a name, an email or a title.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ItemCompletedData {
    #[schema(value_type = String, format = "uuid")]
    pub user_id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub course_id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub module_id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub item_id: Uuid,
    pub item_kind: ItemKind,
    pub requirement_kind: RequirementKind,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub completed_at: String,
}

/// The kind of requirement a completion met.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RequirementKind {
    View,
    MarkDone,
    Watch,
}

impl From<rules::RequirementKind> for RequirementKind {
    fn from(kind: rules::RequirementKind) -> Self {
        match kind {
            rules::RequirementKind::View => Self::View,
            rules::RequirementKind::MarkDone => Self::MarkDone,
            rules::RequirementKind::Watch => Self::Watch,
        }
    }
}

/// The `data` of `module.completed` (B11).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ModuleCompletedData {
    #[schema(value_type = String, format = "uuid")]
    pub user_id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub course_id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub module_id: Uuid,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub completed_at: String,
}

/// The `data` of `course.completed` (B11).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct CourseCompletedData {
    #[schema(value_type = String, format = "uuid")]
    pub user_id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub course_id: Uuid,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub completed_at: String,
}

/// An event's envelope (spec 01 B20) around `data`, as the outbox stores it and as later
/// webhooks deliver it. `$type_name` is an enum of one variant, the topic, written beside the
/// macro call: utoipa does not read a `serde(rename)` a macro passes on.
macro_rules! event_envelope {
    ($name:ident, $type_name:ident, $data:ty, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
        pub struct $name {
            #[serde(rename = "type")]
            pub event_type: $type_name,
            /// RFC 3339, UTC: when the change happened.
            #[schema(format = DateTime)]
            pub timestamp: String,
            pub data: $data,
        }
    };
}

/// The type of an `item.completed` event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub enum ItemCompletedType {
    #[serde(rename = "item.completed")]
    ItemCompleted,
}

/// The type of a `module.completed` event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub enum ModuleCompletedType {
    #[serde(rename = "module.completed")]
    ModuleCompleted,
}

/// The type of a `course.completed` event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub enum CourseCompletedType {
    #[serde(rename = "course.completed")]
    CourseCompleted,
}

event_envelope!(
    ItemCompletedEvent,
    ItemCompletedType,
    ItemCompletedData,
    "`item.completed`, schema version 1: a learner completed an item."
);
event_envelope!(
    ModuleCompletedEvent,
    ModuleCompletedType,
    ModuleCompletedData,
    "`module.completed`, schema version 1: a learner completed a module."
);
event_envelope!(
    CourseCompletedEvent,
    CourseCompletedType,
    CourseCompletedData,
    "`course.completed`, schema version 1: a learner completed a course."
);

/// Write `events` to the outbox in `tx`, the transaction that completed them, at `now` (B10).
/// Every caller of `stepclimb_core::progress::record_step` or `complete_item` passes it what
/// they returned before it commits.
pub async fn emit(
    tx: &mut Transaction<'_, Postgres>,
    scope: OrgScope,
    events: &[Event],
    now: SystemTime,
) -> Result<(), ApiError> {
    let failed = |error: stepclimb_jobs::outbox::Error| ApiError::internal(&error);
    for event in events {
        match event {
            Event::ItemCompleted(e) => {
                let data = ItemCompletedData {
                    user_id: e.user_id,
                    course_id: e.course_id,
                    module_id: e.module_id,
                    item_id: e.item_id,
                    item_kind: e.item_kind.into(),
                    requirement_kind: e.requirement_kind.into(),
                    completed_at: time(e.completed_at)?,
                };
                stepclimb_jobs::outbox::emit(tx, scope, &ITEM_COMPLETED, &data, now)
                    .await
                    .map_err(failed)?;
            }
            Event::ModuleCompleted(e) => {
                let data = ModuleCompletedData {
                    user_id: e.user_id,
                    course_id: e.course_id,
                    module_id: e.module_id,
                    completed_at: time(e.completed_at)?,
                };
                stepclimb_jobs::outbox::emit(tx, scope, &MODULE_COMPLETED, &data, now)
                    .await
                    .map_err(failed)?;
            }
            Event::CourseCompleted(e) => {
                let data = CourseCompletedData {
                    user_id: e.user_id,
                    course_id: e.course_id,
                    completed_at: time(e.completed_at)?,
                };
                stepclimb_jobs::outbox::emit(tx, scope, &COURSE_COMPLETED, &data, now)
                    .await
                    .map_err(failed)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn repository_errors_map_to_their_status_and_code() {
        let cases = [
            (Error::ItemNotFound, 404, "item.not_found"),
            (
                Error::Locked(vec![stepclimb_core::unlock::LockReason::ModuleLocked]),
                403,
                "item.locked",
            ),
            (Error::CourseArchived, 409, "course.archived"),
            (Error::NotLearner, 403, "permission.denied"),
            (Error::ActionInvalid, 422, "request.invalid"),
            (Error::CompletionMismatch, 500, "internal.error"),
        ];
        for (error, status, code) in cases {
            let problem = progress_error(error).problem();
            assert_eq!((problem.status, problem.code.as_str()), (status, code));
        }
        let invalid = progress_error(Error::ActionInvalid).problem();
        let fields: Vec<_> = invalid
            .errors
            .iter()
            .map(|e| (e.field.as_str(), e.code.as_str()))
            .collect();
        assert_eq!(fields, [("/action", "progress.action_invalid")]);
    }

    #[test]
    fn a_step_names_one_known_action() {
        let view: ProgressStep = serde_json::from_value(json!({ "action": "view" })).unwrap();
        assert_eq!(view.action, ProgressAction::View);
        let done: ProgressStep = serde_json::from_value(json!({ "action": "mark_done" })).unwrap();
        assert_eq!(done.action, ProgressAction::MarkDone);
        for refused in [
            json!({ "action": "unmark" }),
            json!({ "action": null }),
            json!({}),
            json!({ "action": "view", "extra": 1 }),
        ] {
            assert!(
                serde_json::from_value::<ProgressStep>(refused.clone()).is_err(),
                "{refused}"
            );
        }
    }

    #[test]
    fn the_registry_knows_the_three_topics_once() {
        let mut registry = Registry::new();
        register_topics(&mut registry).unwrap();
        for topic in [ITEM_COMPLETED, MODULE_COMPLETED, COURSE_COMPLETED] {
            assert!(registry.knows_topic(&topic), "{}", topic.name);
        }
        assert!(register_topics(&mut registry).is_err());
    }

    #[test]
    fn an_envelope_names_its_type() {
        let event = CourseCompletedEvent {
            event_type: CourseCompletedType::CourseCompleted,
            timestamp: "2025-11-13T02:13:20.000000Z".into(),
            data: CourseCompletedData {
                user_id: Uuid::from_u128(1),
                course_id: Uuid::from_u128(2),
                completed_at: "2025-11-13T02:13:20.000000Z".into(),
            },
        };
        assert_eq!(
            serde_json::to_value(event).unwrap()["type"],
            "course.completed"
        );
    }
}
