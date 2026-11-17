// SPDX-License-Identifier: AGPL-3.0-or-later
//! Authoring a course's modules (docs/specs/m1/03-modules-and-items.md B1 to B3, B10, B12 and
//! API): create, rename, publish, reorder and delete. Items are [`crate::items`]; this module
//! holds the schemas and the error mapping both use.
//!
//! Every route takes `Require<perm::CourseContentEdit, _>` at the course (directly, or through
//! the module with [`ModuleTarget`]). Inside its transaction, the handler then locks the course
//! row and its account (`courses::lock`) and checks `course.content_edit` again on the path read
//! there, so an account move committed after the extractor ran cannot carry the course out of
//! the caller's reach unseen. The repository refuses every write to an archived course (409
//! `course.archived`, spec 02 B5).

use std::collections::HashMap;

use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use stepclimb_core::{
    courses,
    grants::Denied,
    id::Uuid,
    items,
    modules::{self, Error},
    perm::{CourseContentEdit, Permission},
    rules,
};
use utoipa::ToSchema;

use crate::{
    AppState,
    courses::{ApiTimestamp, nullable},
    error::{ApiError, FieldError},
    extract::ApiJson,
    openapi::problem_responses,
    organization::present,
    require::{
        CourseTarget, ModuleTarget, Require, check_course, code::COURSE_NOT_FOUND, not_found_as,
    },
    roles::code::{NAME_EMPTY, NAME_TOO_LONG, TEXT_CONTAINS_NUL},
    timestamp,
};

/// The path of a module; `{MODULES_PATH}/{module_id}`.
pub const MODULES_PATH: &str = "/api/v1/modules";

/// The codes this module and [`crate::items`] produce besides the targets' 404s.
pub mod code {
    /// The course is archived and takes no content write (409).
    pub const COURSE_ARCHIVED: &str = "course.archived";
    /// The course holds 200 modules (409).
    pub const MODULE_LIMIT_REACHED: &str = "module.limit_reached";
    /// The module still has items (409).
    pub const MODULE_NOT_EMPTY: &str = "module.not_empty";
    /// The module holds 200 items (409).
    pub const ITEM_LIMIT_REACHED: &str = "item.limit_reached";
    /// A reorder's list is not exactly the current set of ids (409).
    pub const ORDER_STALE: &str = "order.stale";
    /// The kind cannot be created yet (a 422 field code at `/kind`).
    pub const ITEM_KIND_UNAVAILABLE: &str = "item.kind_unavailable";
    /// The URL is not one B7 accepts (a 422 field code at `/url`).
    pub const ITEM_URL_INVALID: &str = "item.url_invalid";
    /// An external link without a URL (a 422 field code at `/url`).
    pub const ITEM_URL_REQUIRED: &str = "item.url_required";
    /// A URL on an item that is not an external link (a 422 field code at `/url`).
    pub const ITEM_URL_UNAVAILABLE: &str = "item.url_unavailable";
    /// A `ref_id` on a kind that has none (a 422 field code at `/ref_id`).
    pub const ITEM_REF_ID_UNAVAILABLE: &str = "item.ref_id_unavailable";
    /// A position outside the module (a 422 field code at `/position`).
    pub const ITEM_POSITION_OUT_OF_RANGE: &str = "item.position_out_of_range";
    /// The prerequisites would form a cycle (409).
    pub const MODULE_PREREQUISITE_CYCLE: &str = "module.prerequisite_cycle";
    /// A module named as its own prerequisite (a 422 field code at `/prerequisite_ids`).
    pub const MODULE_PREREQUISITE_SELF: &str = "module.prerequisite_self";
    /// A prerequisite named twice (a 422 field code at `/prerequisite_ids`).
    pub const MODULE_PREREQUISITE_DUPLICATE: &str = "module.prerequisite_duplicate";
    /// More than 20 prerequisites (a 422 field code at `/prerequisite_ids`).
    pub const MODULE_PREREQUISITES_TOO_MANY: &str = "module.prerequisites_too_many";
    /// An unlock date before 1970 or from 2200 on (a 422 field code at `/unlock_at`).
    pub const MODULE_DATE_OUT_OF_RANGE: &str = "module.date_out_of_range";
    /// A requirement of an unknown shape or on an item kind that does not take it (a 422 field
    /// code at `/requirement`).
    pub const REQUIREMENT_INVALID: &str = "requirement.invalid";
    /// A requirement kind reserved for M2 or M3 (a 422 field code at `/requirement`).
    pub const REQUIREMENT_UNAVAILABLE: &str = "requirement.unavailable";
}

problem_responses! {
    ModuleCourseNotFound => "`course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined.",
    ModuleNotFound => "`module.not_found`: no module with this id exists in the organization, or you cannot see its course; a draft course or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined.",
    ContentEditForbidden => "`permission.denied`: you can see the course but do not hold `course.content_edit` at a scope containing it. `auth.csrf_refused`: the request did not come from this site.",
    ModuleCreateConflict => "`course.archived`: the course is archived and takes no content change. `module.limit_reached`: the course holds 200 modules.",
    ModuleOrderConflict => "`course.archived`: the course is archived and takes no content change. `order.stale`: the list is not exactly the current set of ids (one is missing, extra or repeated); reload and try again.",
    ModuleWriteConflict => "`course.archived`: the course is archived and takes no content change.",
    ModulePatchNotFound => "`module.not_found`: no module with this id exists in the organization, or you cannot see its course, or a `prerequisite_ids` entry is not a module of the same course. `org.not_found`: the request's organization cannot be determined.",
    ModulePatchConflict => "`course.archived`: the course is archived and takes no content change. `module.prerequisite_cycle`: the prerequisites would form a cycle in the course's modules.",
    ModulePatchInvalid => "`request.invalid`: `errors` names `/title` (`name.empty`, `name.too_long`, `text.contains_nul`), `/unlock_at` with `module.date_out_of_range` (before 1970 or from 2200 on), or `/prerequisite_ids` with `module.prerequisite_self` (the module itself), `module.prerequisite_duplicate` (an id named twice) or `module.prerequisites_too_many` (more than 20).",
    ModuleDeleteConflict => "`course.archived`: the course is archived and takes no content change. `module.not_empty`: the module has items; delete or move them first.",
}

/// What an item points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Header,
    ExternalLink,
    File,
    Page,
    Video,
    Quiz,
    Activity,
}

impl From<items::Kind> for ItemKind {
    fn from(kind: items::Kind) -> Self {
        match kind {
            items::Kind::Header => Self::Header,
            items::Kind::ExternalLink => Self::ExternalLink,
            items::Kind::File => Self::File,
            items::Kind::Page => Self::Page,
            items::Kind::Video => Self::Video,
            items::Kind::Quiz => Self::Quiz,
            items::Kind::Activity => Self::Activity,
        }
    }
}

impl From<ItemKind> for items::Kind {
    fn from(kind: ItemKind) -> Self {
        match kind {
            ItemKind::Header => Self::Header,
            ItemKind::ExternalLink => Self::ExternalLink,
            ItemKind::File => Self::File,
            ItemKind::Page => Self::Page,
            ItemKind::Video => Self::Video,
            ItemKind::Quiz => Self::Quiz,
            ItemKind::Activity => Self::Activity,
        }
    }
}

/// Whether all or one of a module's requirements complete it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompletionRule {
    All,
    One,
}

impl From<rules::CompletionRule> for CompletionRule {
    fn from(rule: rules::CompletionRule) -> Self {
        match rule {
            rules::CompletionRule::All => Self::All,
            rules::CompletionRule::One => Self::One,
        }
    }
}

impl From<CompletionRule> for rules::CompletionRule {
    fn from(rule: CompletionRule) -> Self {
        match rule {
            CompletionRule::All => Self::All,
            CompletionRule::One => Self::One,
        }
    }
}

/// What completes an item for a learner: `view` (open it) and `mark_done` (mark it done) on a
/// page, file, external link or video; `watch` (at least `min_percent`, 1 to 100, of a video).
/// `submit`, `score` and `mastery` are reserved for quizzes and activities.
//
// Serialized only: a request's requirement is read as JSON and checked by
// `stepclimb_core::rules::Requirement::parse`, which answers `requirement.invalid` and
// `requirement.unavailable` rather than a schema mismatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Requirement {
    View,
    MarkDone,
    Watch {
        /// 1 to 100.
        min_percent: u8,
    },
}

impl From<rules::Requirement> for Requirement {
    fn from(requirement: rules::Requirement) -> Self {
        match requirement {
            rules::Requirement::View => Self::View,
            rules::Requirement::MarkDone => Self::MarkDone,
            rules::Requirement::Watch { min_percent } => Self::Watch { min_percent },
        }
    }
}

/// An item of a module.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Item {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub module_id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub course_id: Uuid,
    pub kind: ItemKind,
    pub title: String,
    /// 0-based, dense within the module.
    pub position: i32,
    pub published: bool,
    /// What the item points at; only for kinds that point at a row.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>, format = "uuid", required = false, nullable = false)]
    pub ref_id: Option<Uuid>,
    /// Only for `external_link`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false, nullable = false)]
    pub url: Option<String>,
    /// What completes the item for a learner.
    #[schema(required = true)]
    pub requirement: Option<Requirement>,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub updated_at: String,
}

impl Item {
    pub(crate) fn from_repo(item: items::Item) -> Result<Self, ApiError> {
        Ok(Self {
            created_at: time(item.created_at)?,
            updated_at: time(item.updated_at)?,
            kind: item.kind.into(),
            id: item.id,
            module_id: item.module_id,
            course_id: item.course_id,
            title: item.title,
            position: item.position,
            published: item.published,
            requirement: item.requirement.map(Into::into),
            ref_id: item.ref_id,
            url: item.url,
        })
    }
}

/// A module of a course, with its items in order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Module {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub course_id: Uuid,
    pub title: String,
    /// 0-based, dense within the course.
    pub position: i32,
    pub published: bool,
    /// RFC 3339, UTC: the module stays locked to learners before this time.
    #[schema(format = DateTime, required = true)]
    pub unlock_at: Option<String>,
    /// The modules of the course this one waits for, in course order.
    #[schema(value_type = Vec<String>)]
    pub prerequisite_ids: Vec<Uuid>,
    /// Whether the module's items go in order.
    pub require_sequential: bool,
    /// Whether all or one of the module's requirements complete it.
    pub completion_rule: CompletionRule,
    pub items: Vec<Item>,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub updated_at: String,
}

impl Module {
    pub(crate) fn from_repo(
        module: modules::Module,
        items: Vec<items::Item>,
    ) -> Result<Self, ApiError> {
        Ok(Self {
            items: items
                .into_iter()
                .map(Item::from_repo)
                .collect::<Result<_, _>>()?,
            unlock_at: module.unlock_at.map(time).transpose()?,
            completion_rule: module.completion_rule.into(),
            created_at: time(module.created_at)?,
            updated_at: time(module.updated_at)?,
            id: module.id,
            course_id: module.course_id,
            title: module.title,
            position: module.position,
            published: module.published,
            prerequisite_ids: module.prerequisite_ids,
            require_sequential: module.require_sequential,
        })
    }
}

fn time(t: std::time::SystemTime) -> Result<String, ApiError> {
    timestamp::rfc3339(t).map_err(|error| ApiError::internal(&error))
}

/// A course's modules, in order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ModuleList {
    pub modules: Vec<Module>,
}

/// A module to create, unpublished and last.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewModule {
    /// Trimmed; 1 to 200 characters.
    pub title: String,
}

/// A change to a module; an omitted member is left as it is.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ModulePatch {
    /// Trimmed; 1 to 200 characters.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub title: Option<String>,
    /// Whether learners see the module.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = bool)]
    pub published: Option<bool>,
    /// RFC 3339 with an offset, from 1970 up to 2200; `null` clears it.
    #[serde(default, deserialize_with = "nullable")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub unlock_at: Option<Option<ApiTimestamp>>,
    /// The whole new set of modules of the same course this one waits for: not itself, each
    /// once, at most 20, and closing no cycle.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Vec<String>)]
    pub prerequisite_ids: Option<Vec<Uuid>>,
    /// Whether the module's items go in order.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = bool)]
    pub require_sequential: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = CompletionRule)]
    pub completion_rule: Option<CompletionRule>,
}

impl ModulePatch {
    fn rules(&self) -> modules::RulesPatch {
        modules::RulesPatch {
            unlock_at: self.unlock_at.map(|at| at.map(ApiTimestamp::time)),
            prerequisite_ids: self.prerequisite_ids.clone(),
            require_sequential: self.require_sequential,
            completion_rule: self.completion_rule.map(Into::into),
        }
    }
}

/// Every module id of the course, in the new order.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ModuleOrder {
    #[schema(value_type = Vec<String>)]
    pub module_ids: Vec<Uuid>,
}

/// Create an unpublished module, last in the course.
#[utoipa::path(
    post,
    path = "/api/v1/courses/{course_id}/modules",
    operation_id = "createModule",
    tag = "modules",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("course_id" = String, Path, format = "uuid", description = "The course's id.")),
    request_body = NewModule,
    responses(
        (
            status = 201,
            description = "The module is created",
            body = Module,
            headers(("Location" = String, description = "The module's path"))
        ),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ContentEditForbidden),
        (status = 404, response = ModuleCourseNotFound),
        (status = 409, response = ModuleCreateConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn create_module(
    require: Require<CourseContentEdit, CourseTarget>,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<NewModule>,
) -> Result<Response, ApiError> {
    let (grants, course_id) = (require.grants(), require.target().id);
    let mut tx = begin(&state).await?;
    recheck(&mut tx, &require, course_id, COURSE_NOT_FOUND).await?;
    let module = modules::create(
        &mut *tx,
        grants.scope(),
        course_id,
        &body.title,
        state.clock.now(),
    )
    .await
    .map_err(content_error)?;
    commit(tx).await?;
    let location = format!("{MODULES_PATH}/{}", module.id);
    let module = Module::from_repo(module, Vec::new())?;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(module),
    )
        .into_response())
}

/// Put the course's modules in a new order. The list must be exactly the course's current
/// module ids; the current order changes nothing.
#[utoipa::path(
    put,
    path = "/api/v1/courses/{course_id}/modules/order",
    operation_id = "reorderModules",
    tag = "modules",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("course_id" = String, Path, format = "uuid", description = "The course's id.")),
    request_body = ModuleOrder,
    responses(
        (status = 200, description = "The course's modules in their new order", body = ModuleList),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ContentEditForbidden),
        (status = 404, response = ModuleCourseNotFound),
        (status = 409, response = ModuleOrderConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn reorder_modules(
    require: Require<CourseContentEdit, CourseTarget>,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<ModuleOrder>,
) -> Result<Json<ModuleList>, ApiError> {
    let (grants, course_id) = (require.grants(), require.target().id);
    let mut tx = begin(&state).await?;
    recheck(&mut tx, &require, course_id, COURSE_NOT_FOUND).await?;
    let scope = grants.scope();
    let ordered = modules::reorder(&mut *tx, scope, course_id, &body.module_ids)
        .await
        .map_err(content_error)?;
    let all_items = items::list_for_course(&mut *tx, scope, course_id)
        .await
        .map_err(content_error)?;
    commit(tx).await?;
    let mut by_module: HashMap<Uuid, Vec<items::Item>> = HashMap::new();
    for item in all_items {
        by_module.entry(item.module_id).or_default().push(item);
    }
    let modules = ordered
        .into_iter()
        .map(|m| {
            let items = by_module.remove(&m.id).unwrap_or_default();
            Module::from_repo(m, items)
        })
        .collect::<Result<_, _>>()?;
    Ok(Json(ModuleList { modules }))
}

/// Rename, publish or unpublish a module, or change its rules: unlock date, prerequisites,
/// sequential order and completion rule. A patch naming only current values changes nothing.
#[utoipa::path(
    patch,
    path = "/api/v1/modules/{module_id}",
    operation_id = "updateModule",
    tag = "modules",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("module_id" = String, Path, format = "uuid", description = "The module's id.")),
    request_body = ModulePatch,
    responses(
        (status = 200, description = "The module after the change", body = Module),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ContentEditForbidden),
        (status = 404, response = ModulePatchNotFound),
        (status = 409, response = ModulePatchConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = ModulePatchInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_module(
    require: Require<CourseContentEdit, ModuleTarget>,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<ModulePatch>,
) -> Result<Json<Module>, ApiError> {
    let (grants, target) = (require.grants(), require.target());
    let mut tx = begin(&state).await?;
    recheck(&mut tx, &require, target.course.id, MODULE_NOT_FOUND).await?;
    let patch = modules::ModulePatch {
        title: body.title.clone(),
        published: body.published,
    };
    let (scope, now) = (grants.scope(), state.clock.now());
    modules::update(&mut *tx, scope, target.id, &patch, now)
        .await
        .map_err(content_error)?;
    let module = modules::set_rules(&mut *tx, scope, target.id, &body.rules(), now)
        .await
        .map_err(content_error)?;
    let items = items::list_for_module(&mut *tx, scope, target.id)
        .await
        .map_err(content_error)?;
    commit(tx).await?;
    Ok(Json(Module::from_repo(module, items)?))
}

/// Delete a module that has no items.
#[utoipa::path(
    delete,
    path = "/api/v1/modules/{module_id}",
    operation_id = "deleteModule",
    tag = "modules",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("module_id" = String, Path, format = "uuid", description = "The module's id.")),
    responses(
        (status = 204, description = "The module is deleted"),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ContentEditForbidden),
        (status = 404, response = ModuleNotFound),
        (status = 409, response = ModuleDeleteConflict),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn delete_module(
    require: Require<CourseContentEdit, ModuleTarget>,
    State(state): State<AppState>,
) -> Result<StatusCode, ApiError> {
    let (grants, target) = (require.grants(), require.target());
    let mut tx = begin(&state).await?;
    recheck(&mut tx, &require, target.course.id, MODULE_NOT_FOUND).await?;
    modules::delete(&mut *tx, grants.scope(), target.id)
        .await
        .map_err(content_error)?;
    commit(tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The code of a module that does not exist or that the caller cannot see.
pub(crate) const MODULE_NOT_FOUND: &str = crate::require::code::MODULE_NOT_FOUND;

/// `require`'s check of `P` again, on the course's path as `courses::lock` reads it in the
/// transaction: 404 with `not_found` when the course is gone or out of the caller's reach, 403
/// when it is visible without `P`. `P` is the permission the route's own `Require` names, so the
/// two checks cannot differ. The course row stays locked until the transaction ends; the course
/// as it was locked is returned for a caller that reads its status.
pub(crate) async fn recheck<P: Permission, T>(
    tx: &mut Transaction<'static, Postgres>,
    require: &Require<P, T>,
    course_id: Uuid,
    not_found: &'static str,
) -> Result<courses::Course, ApiError> {
    let locked = courses::lock(tx, require.grants().scope(), course_id).await;
    check_locked(require, course_id, locked, not_found)
}

/// [`recheck`] with the course row `FOR SHARE` (`courses::lock_shared`): a status change waits
/// for the transaction, but other writes that take it this way do not wait for each other.
pub(crate) async fn recheck_shared<P: Permission, T>(
    tx: &mut Transaction<'static, Postgres>,
    require: &Require<P, T>,
    course_id: Uuid,
    not_found: &'static str,
) -> Result<courses::Course, ApiError> {
    let locked = courses::lock_shared(tx, require.grants().scope(), course_id).await;
    check_locked(require, course_id, locked, not_found)
}

/// The check of [`recheck`] on the course as its transaction locked it.
fn check_locked<P: Permission, T>(
    require: &Require<P, T>,
    course_id: Uuid,
    locked: Result<Option<courses::Course>, courses::Error>,
    not_found: &'static str,
) -> Result<courses::Course, ApiError> {
    let course = locked
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(|| not_found_as(Denied::CourseNotFound, not_found))?;
    let target = CourseTarget {
        id: course_id,
        account_path: course.account_path.clone(),
    };
    check_course::<P>(
        require.grants(),
        &target,
        course.needs_view_all(),
        not_found,
    )?;
    Ok(course)
}

pub(crate) async fn begin(state: &AppState) -> Result<Transaction<'static, Postgres>, ApiError> {
    state
        .db
        .begin()
        .await
        .map_err(|error| ApiError::internal(&error))
}

pub(crate) async fn commit(tx: Transaction<'static, Postgres>) -> Result<(), ApiError> {
    tx.commit()
        .await
        .map_err(|error| ApiError::internal(&error))
}

/// The answer for a modules or items repository error.
pub(crate) fn content_error(error: Error) -> ApiError {
    let invalid = |field: &str, code| ApiError::invalid(vec![FieldError::at(&[field], code)]);
    let conflict = |code, detail| ApiError::new(StatusCode::CONFLICT, code, detail);
    let not_found = |code| not_found_as(Denied::CourseNotFound, code);
    match error {
        Error::CourseNotFound => not_found(COURSE_NOT_FOUND),
        Error::ModuleNotFound => not_found(MODULE_NOT_FOUND),
        Error::ItemNotFound => not_found(crate::require::code::ITEM_NOT_FOUND),
        Error::CourseArchived => conflict(
            code::COURSE_ARCHIVED,
            "The course is archived and takes no content change.",
        ),
        Error::ModuleLimitReached => conflict(
            code::MODULE_LIMIT_REACHED,
            "The course holds the most modules it may.",
        ),
        Error::ItemLimitReached => conflict(
            code::ITEM_LIMIT_REACHED,
            "The module holds the most items it may.",
        ),
        Error::ModuleNotEmpty => conflict(code::MODULE_NOT_EMPTY, "The module has items."),
        Error::OrderStale => conflict(
            code::ORDER_STALE,
            "The list is not the current set of ids; reload and try again.",
        ),
        Error::TitleEmpty => invalid("title", NAME_EMPTY),
        Error::TitleTooLong => invalid("title", NAME_TOO_LONG),
        Error::ContainsNul { field } => invalid(field, TEXT_CONTAINS_NUL),
        Error::KindUnavailable => invalid("kind", code::ITEM_KIND_UNAVAILABLE),
        Error::UrlInvalid => invalid("url", code::ITEM_URL_INVALID),
        Error::UrlRequired => invalid("url", code::ITEM_URL_REQUIRED),
        Error::UrlUnavailable => invalid("url", code::ITEM_URL_UNAVAILABLE),
        Error::RefIdUnavailable => invalid("ref_id", code::ITEM_REF_ID_UNAVAILABLE),
        Error::PositionOutOfRange => invalid("position", code::ITEM_POSITION_OUT_OF_RANGE),
        Error::PrerequisiteNotFound => not_found(MODULE_NOT_FOUND),
        Error::PrerequisiteCycle => conflict(
            code::MODULE_PREREQUISITE_CYCLE,
            "The prerequisites would form a cycle in the course's modules.",
        ),
        Error::PrerequisiteSelf => invalid("prerequisite_ids", code::MODULE_PREREQUISITE_SELF),
        Error::PrerequisiteDuplicate => {
            invalid("prerequisite_ids", code::MODULE_PREREQUISITE_DUPLICATE)
        }
        Error::PrerequisitesTooMany => {
            invalid("prerequisite_ids", code::MODULE_PREREQUISITES_TOO_MANY)
        }
        Error::UnlockAtOutOfRange => invalid("unlock_at", code::MODULE_DATE_OUT_OF_RANGE),
        Error::RequirementInvalid => invalid("requirement", code::REQUIREMENT_INVALID),
        Error::RequirementUnavailable => invalid("requirement", code::REQUIREMENT_UNAVAILABLE),
        other @ (Error::Stored { .. }
        | Error::IsolationLevel { .. }
        | Error::Time(_)
        | Error::Database(_)) => ApiError::internal(&other),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn repository_errors_map_to_their_status_code_and_field() {
        let cases = [
            (Error::CourseNotFound, 404, "course.not_found", None),
            (Error::ModuleNotFound, 404, "module.not_found", None),
            (Error::ItemNotFound, 404, "item.not_found", None),
            (Error::CourseArchived, 409, "course.archived", None),
            (Error::ModuleLimitReached, 409, "module.limit_reached", None),
            (Error::ItemLimitReached, 409, "item.limit_reached", None),
            (Error::ModuleNotEmpty, 409, "module.not_empty", None),
            (Error::OrderStale, 409, "order.stale", None),
            (
                Error::TitleEmpty,
                422,
                "request.invalid",
                Some(("/title", "name.empty")),
            ),
            (
                Error::TitleTooLong,
                422,
                "request.invalid",
                Some(("/title", "name.too_long")),
            ),
            (
                Error::KindUnavailable,
                422,
                "request.invalid",
                Some(("/kind", "item.kind_unavailable")),
            ),
            (
                Error::UrlInvalid,
                422,
                "request.invalid",
                Some(("/url", "item.url_invalid")),
            ),
            (
                Error::UrlRequired,
                422,
                "request.invalid",
                Some(("/url", "item.url_required")),
            ),
            (
                Error::UrlUnavailable,
                422,
                "request.invalid",
                Some(("/url", "item.url_unavailable")),
            ),
            (
                Error::RefIdUnavailable,
                422,
                "request.invalid",
                Some(("/ref_id", "item.ref_id_unavailable")),
            ),
            (
                Error::PositionOutOfRange,
                422,
                "request.invalid",
                Some(("/position", "item.position_out_of_range")),
            ),
            (Error::PrerequisiteNotFound, 404, "module.not_found", None),
            (
                Error::PrerequisiteCycle,
                409,
                "module.prerequisite_cycle",
                None,
            ),
            (
                Error::PrerequisiteSelf,
                422,
                "request.invalid",
                Some(("/prerequisite_ids", "module.prerequisite_self")),
            ),
            (
                Error::PrerequisiteDuplicate,
                422,
                "request.invalid",
                Some(("/prerequisite_ids", "module.prerequisite_duplicate")),
            ),
            (
                Error::PrerequisitesTooMany,
                422,
                "request.invalid",
                Some(("/prerequisite_ids", "module.prerequisites_too_many")),
            ),
            (
                Error::UnlockAtOutOfRange,
                422,
                "request.invalid",
                Some(("/unlock_at", "module.date_out_of_range")),
            ),
            (
                Error::RequirementInvalid,
                422,
                "request.invalid",
                Some(("/requirement", "requirement.invalid")),
            ),
            (
                Error::RequirementUnavailable,
                422,
                "request.invalid",
                Some(("/requirement", "requirement.unavailable")),
            ),
            (
                Error::ContainsNul { field: "url" },
                422,
                "request.invalid",
                Some(("/url", "text.contains_nul")),
            ),
        ];
        for (error, status, code, field) in cases {
            let problem = content_error(error).problem();
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
    fn a_module_patch_refuses_null() {
        for refused in [
            json!({ "title": null }),
            json!({ "published": null }),
            json!({ "position": 1 }),
            json!({ "prerequisite_ids": null }),
            json!({ "require_sequential": null }),
            json!({ "completion_rule": null }),
            json!({ "completion_rule": "any" }),
            json!({ "unlock_at": "2026-01-02" }),
        ] {
            assert!(
                serde_json::from_value::<ModulePatch>(refused.clone()).is_err(),
                "{refused}"
            );
        }
        let empty: ModulePatch = serde_json::from_value(json!({})).unwrap();
        assert_eq!(empty.rules(), modules::RulesPatch::default());
        assert_eq!((empty.title, empty.published), (None, None));
        let cleared: ModulePatch = serde_json::from_value(json!({ "unlock_at": null })).unwrap();
        assert_eq!(cleared.rules().unlock_at, Some(None));
    }
}
