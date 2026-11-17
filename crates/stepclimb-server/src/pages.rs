// SPDX-License-Identifier: AGPL-3.0-or-later
//! Authoring a course's pages (docs/specs/m1/07-pages-and-the-editor.md B1 to B3, B5 to B13 and
//! API): list and create a course's pages, rename one, save its body, list, read and restore its
//! versions, and delete it. Reading a page as a learner, rendered, is spec 07 I2.
//!
//! The reads take `Require<perm::CourseViewAll, _>` and the writes `Require<perm::CourseContentEdit,
//! _>`, at the course directly or through the page ([`PageTarget`]); a draft or template course
//! answers 404 without `course.view_all` (`require::check_course`). Each write checks its
//! permission again under the course lock (`modules::recheck`). A body is parsed into
//! `stepclimb_core::page_doc::PageDoc` before the transaction starts, so a body the schema does
//! not allow answers 422 with the JSON Pointer of its first offense and writes nothing.

use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use stepclimb_core::{
    grants::Denied,
    id::Uuid,
    page_doc::{Invalid, PageDoc},
    pages::{self, Error, Slug},
    perm::{CourseContentEdit, CourseViewAll},
};
use utoipa::ToSchema;

use crate::{
    AppState,
    error::{ApiError, CurrentVersion, FieldError},
    extract::{ApiJson, ApiPath, ApiQuery},
    modules::{begin, commit, recheck},
    openapi::problem_responses,
    organization::present,
    pagination::{Page, PageParams},
    require::{
        CourseTarget, PageTarget, Require,
        code::{COURSE_NOT_FOUND, FILE_NOT_FOUND, PAGE_NOT_FOUND},
        not_found_as,
    },
    roles::code::{NAME_EMPTY, NAME_TOO_LONG, TEXT_CONTAINS_NUL},
    timestamp,
};

/// The path of a page; `{PAGES_PATH}/{page_id}`.
pub const PAGES_PATH: &str = "/api/v1/pages";

/// The codes this module produces besides the targets' 404s.
pub mod code {
    /// The course already has a page with the slug (409).
    pub const SLUG_TAKEN: &str = "page.slug_taken";
    /// The save's base version is not the page's current one (409).
    pub const VERSION_CONFLICT: &str = "page.version_conflict";
    /// An item references the page (409).
    pub const IN_USE: &str = "page.in_use";
    /// No version with the id of the page (404).
    pub const VERSION_NOT_FOUND: &str = "page_version.not_found";
    /// A slug that is not one, or none derivable from the title (a 422 field code at `/slug`).
    pub const SLUG_INVALID: &str = "page.slug_invalid";
    /// A body the page schema does not allow (a 422 field code at the first offense).
    pub const BODY_INVALID: &str = "page.body_invalid";
    /// An image naming a file that is not a PNG, JPEG, GIF or WebP image (a 422 field code at
    /// the image's `course_file_id`).
    pub const IMAGE_TYPE: &str = "page.image_type";
}

problem_responses! {
    PagesCourseNotFound => "`course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `file.not_found`: an image of the body names no course file of this course. `org.not_found`: the request's organization cannot be determined.",
    PageNotFound => "`page.not_found`: no page with this id exists in the organization, or you cannot see its course; a draft course or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined.",
    PageSaveNotFound => "`page.not_found`: no page with this id exists in the organization, or you cannot see its course. `file.not_found`: an image of the body names no course file of the page's course. `org.not_found`: the request's organization cannot be determined.",
    PageVersionNotFound => "`page.not_found`: no page with this id exists in the organization, or you cannot see its course. `page_version.not_found`: the page has no version with this id. `file.not_found`: an image of the version names a course file that no longer exists. `org.not_found`: the request's organization cannot be determined.",
    PagesViewForbidden => "`permission.denied`: you can see the course but do not hold `course.view_all` at a scope containing it.",
    PageCreateConflict => "`course.archived`: the course is archived and takes no content change. `page.slug_taken`: the course already has a page with this slug.",
    PagePatchConflict => "`course.archived`: the course is archived and takes no content change. `page.slug_taken`: the course already has another page with this slug.",
    PageSaveConflict => "`course.archived`: the course is archived and takes no content change. `page.version_conflict`: `base_version_id` is not the page's current version, which `current_version` names; nothing was saved. Reload, merge and save again.",
    PageDeleteConflict => "`course.archived`: the course is archived and takes no content change. `page.in_use`: an item references the page; remove it first.",
    PageInvalid => "`request.invalid`: `errors` names `/title` (`name.empty`, `name.too_long`, `text.contains_nul`), `/slug` with `page.slug_invalid` (not lowercase letters and digits in runs joined by single hyphens, 1 to 100 characters, or, with no slug given, a title from which none can be derived), or a member of `/body` with `page.body_invalid` (the first node, mark or attribute the page schema does not allow, or `/body` itself for a body over 1 MiB or 20,000 nodes) or `page.image_type` (an image's `course_file_id` names a file whose type, read from its bytes, is not PNG, JPEG, GIF or WebP).",
    PageRestoreInvalid => "`request.invalid`: `errors` names a member of `/body`, the version's body, with `page.image_type` (an image names a file whose type is not PNG, JPEG, GIF or WebP) or `page.body_invalid` (a node the page schema no longer allows).",
}

/// A version of a page, without its body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct PageVersionSummary {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    /// 1, 2, ... per page.
    pub number: i32,
    /// The user who saved it.
    #[schema(value_type = String, format = "uuid")]
    pub author_id: Uuid,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
}

impl PageVersionSummary {
    fn from_repo(version: pages::VersionSummary) -> Result<Self, ApiError> {
        Ok(Self {
            created_at: time(version.created_at)?,
            id: version.id,
            number: version.number,
            author_id: version.author_id,
        })
    }
}

/// A page of a course with its current version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct PageSummary {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub course_id: Uuid,
    pub title: String,
    /// Unique in the course.
    pub slug: String,
    pub current_version: PageVersionSummary,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
    /// RFC 3339, UTC: the last change to the title, the slug or the current version.
    #[schema(format = DateTime)]
    pub updated_at: String,
}

impl PageSummary {
    fn from_repo(page: pages::Page) -> Result<Self, ApiError> {
        Ok(Self {
            current_version: PageVersionSummary::from_repo(page.current)?,
            created_at: time(page.created_at)?,
            updated_at: time(page.updated_at)?,
            id: page.id,
            course_id: page.course_id,
            title: page.title,
            slug: page.slug,
        })
    }
}

/// A version of a page with its body.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct PageVersion {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub number: i32,
    #[schema(value_type = String, format = "uuid")]
    pub author_id: Uuid,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
    #[schema(value_type = crate::page_doc_schema::PageDoc)]
    pub body: Value,
}

fn time(t: std::time::SystemTime) -> Result<String, ApiError> {
    timestamp::rfc3339(t).map_err(|error| ApiError::internal(&error))
}

/// A page to create, with version 1 of its body.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewPage {
    /// Trimmed; 1 to 200 characters.
    pub title: String,
    /// Lowercase letters and digits in runs joined by single hyphens, 1 to 100 characters,
    /// unique in the course. Derived from the title when omitted.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub slug: Option<String>,
    /// Version 1's body; a document with one empty paragraph when omitted.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = crate::page_doc_schema::PageDoc)]
    pub body: Option<Value>,
}

/// A change to a page; an omitted member is left as it is.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PagePatch {
    /// Trimmed; 1 to 200 characters.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub title: Option<String>,
    /// As for a new page; unique in the course.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub slug: Option<String>,
}

/// A body to save as the page's new current version.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageBodySave {
    #[schema(value_type = crate::page_doc_schema::PageDoc)]
    pub body: Value,
    /// The version the body was edited from; it must still be the page's current version.
    #[schema(value_type = String, format = "uuid")]
    pub base_version_id: Uuid,
}

/// The version a restore was started from.
#[derive(Clone, Copy, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageRestore {
    /// The page's current version as the caller last saw it.
    #[schema(value_type = String, format = "uuid")]
    pub base_version_id: Uuid,
}

/// The path of one version of a page.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct VersionPath {
    pub page_id: Uuid,
    pub version_id: Uuid,
}

/// `value` as a page body to save, or the 422 naming its first offense under `/body` (`/body`
/// itself when its stored form is over 1 MiB).
fn body(value: &Value) -> Result<PageDoc, ApiError> {
    PageDoc::parse_new(value).map_err(|invalid| body_invalid(&invalid))
}

fn body_invalid(invalid: &Invalid) -> ApiError {
    in_body(&invalid.path, code::BODY_INVALID)
}

/// 422 with `code` at `path` under `/body`.
fn in_body(path: &[String], code: &'static str) -> ApiError {
    let mut segments = vec!["body"];
    segments.extend(path.iter().map(String::as_str));
    ApiError::invalid(vec![FieldError::at(&segments, code)])
}

/// `value` as a slug, or the 422 that names `/slug`.
fn slug(value: &str) -> Result<Slug, ApiError> {
    Slug::parse(value).ok_or_else(slug_invalid)
}

fn slug_invalid() -> ApiError {
    ApiError::invalid(vec![FieldError::at(&["slug"], code::SLUG_INVALID)])
}

/// The answer for a pages repository error.
pub(crate) fn page_error(error: Error) -> ApiError {
    let invalid = |field: &str, code| ApiError::invalid(vec![FieldError::at(&[field], code)]);
    let conflict = |code, detail| ApiError::new(StatusCode::CONFLICT, code, detail);
    let not_found = |code| not_found_as(Denied::CourseNotFound, code);
    match error {
        Error::CourseNotFound => not_found(COURSE_NOT_FOUND),
        Error::NotFound => not_found(PAGE_NOT_FOUND),
        Error::FileNotFound => not_found(FILE_NOT_FOUND),
        Error::VersionNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            code::VERSION_NOT_FOUND,
            "The page has no version with this id.",
        ),
        Error::CourseArchived => conflict(
            crate::modules::code::COURSE_ARCHIVED,
            "The course is archived and takes no content change.",
        ),
        Error::SlugTaken => conflict(
            code::SLUG_TAKEN,
            "The course already has a page with this slug.",
        ),
        Error::InUse => conflict(
            code::IN_USE,
            "An item references this page; remove it first.",
        ),
        Error::VersionConflict(current) => conflict(
            code::VERSION_CONFLICT,
            "The page has a newer version than the one this save started from; nothing was saved.",
        )
        .with_current_version(CurrentVersion {
            id: current.id,
            number: current.number,
            author_id: current.author_id,
        }),
        Error::SlugUnderivable => slug_invalid(),
        Error::TitleEmpty => invalid("title", NAME_EMPTY),
        Error::TitleTooLong => invalid("title", NAME_TOO_LONG),
        Error::TitleContainsNul => invalid("title", TEXT_CONTAINS_NUL),
        Error::ImageType { path } => in_body(&path, code::IMAGE_TYPE),
        Error::Body(invalid) => body_invalid(&invalid),
        other @ (Error::Stored { .. } | Error::Time(_) | Error::Database(_)) => {
            ApiError::internal(&other)
        }
    }
}

/// List the course's pages by slug.
#[utoipa::path(
    get,
    path = "/api/v1/courses/{course_id}/pages",
    operation_id = "listPages",
    tag = "pages",
    params(
        ("course_id" = String, Path, format = "uuid", description = "The course's id."),
        PageParams,
    ),
    responses(
        (status = 200, description = "One page of the course's pages", body = Page<PageSummary>),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = PagesViewForbidden),
        (status = 404, response = crate::modules::ModuleCourseNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_pages(
    require: Require<CourseViewAll, CourseTarget>,
    State(state): State<AppState>,
    ApiQuery(page): ApiQuery<PageParams>,
) -> Result<Json<Page<PageSummary>>, ApiError> {
    let page = page.parse::<String>()?;
    let after = page.after().map(String::as_str);
    // PostgreSQL text cannot hold NUL, and the cursor is the caller's string.
    if after.is_some_and(|slug| slug.contains('\0')) {
        return Err(ApiError::invalid_cursor());
    }
    let rows = pages::list(
        &state.db,
        require.grants().scope(),
        require.target().id,
        after,
        page.fetch_limit(),
    )
    .await
    .map_err(page_error)?
    .into_iter()
    .map(PageSummary::from_repo)
    .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(page.page(rows, |row| row.slug.clone())?))
}

/// Create a page with version 1 of its body.
#[utoipa::path(
    post,
    path = "/api/v1/courses/{course_id}/pages",
    operation_id = "createPage",
    tag = "pages",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("course_id" = String, Path, format = "uuid", description = "The course's id.")),
    request_body = NewPage,
    responses(
        (
            status = 201,
            description = "The page is created",
            body = PageSummary,
            headers(("Location" = String, description = "The page's path"))
        ),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::modules::ContentEditForbidden),
        (status = 404, response = PagesCourseNotFound),
        (status = 409, response = PageCreateConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = PageInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn create_page(
    require: Require<CourseContentEdit, CourseTarget>,
    State(state): State<AppState>,
    ApiJson(request): ApiJson<NewPage>,
) -> Result<Response, ApiError> {
    let new = pages::NewPage {
        slug: request.slug.as_deref().map(slug).transpose()?,
        body: match &request.body {
            Some(value) => body(value)?,
            None => PageDoc::empty(),
        },
        title: request.title,
    };
    let course_id = require.target().id;
    let mut tx = begin(&state).await?;
    recheck(&mut tx, &require, course_id, COURSE_NOT_FOUND).await?;
    let page = pages::create(
        &mut tx,
        require.grants().scope(),
        course_id,
        &new,
        require.principal().user_id,
        state.clock.now(),
    )
    .await
    .map_err(page_error)?;
    commit(tx).await?;
    let location = format!("{PAGES_PATH}/{}", page.id);
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(PageSummary::from_repo(page)?),
    )
        .into_response())
}

/// Rename a page or change its slug. A patch naming only current values changes nothing.
#[utoipa::path(
    patch,
    path = "/api/v1/pages/{page_id}",
    operation_id = "updatePage",
    tag = "pages",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("page_id" = String, Path, format = "uuid", description = "The page's id.")),
    request_body = PagePatch,
    responses(
        (status = 200, description = "The page after the change", body = PageSummary),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::modules::ContentEditForbidden),
        (status = 404, response = PageNotFound),
        (status = 409, response = PagePatchConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = PageInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_page(
    require: Require<CourseContentEdit, PageTarget>,
    State(state): State<AppState>,
    ApiJson(request): ApiJson<PagePatch>,
) -> Result<Json<PageSummary>, ApiError> {
    let patch = pages::PagePatch {
        slug: request.slug.as_deref().map(slug).transpose()?,
        title: request.title,
    };
    let target = require.target();
    let mut tx = begin(&state).await?;
    recheck(&mut tx, &require, target.course.id, PAGE_NOT_FOUND).await?;
    let page = pages::update(
        &mut tx,
        require.grants().scope(),
        target.id,
        &patch,
        state.clock.now(),
    )
    .await
    .map_err(page_error)?;
    commit(tx).await?;
    Ok(Json(PageSummary::from_repo(page)?))
}

/// Save a body as the page's new current version. The save is refused, writing nothing, when
/// `base_version_id` is no longer the current version; a body equal to the current one writes
/// nothing and answers the page as it is.
#[utoipa::path(
    put,
    path = "/api/v1/pages/{page_id}/body",
    operation_id = "savePageBody",
    tag = "pages",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("page_id" = String, Path, format = "uuid", description = "The page's id.")),
    request_body = PageBodySave,
    responses(
        (status = 200, description = "The page with its current version", body = PageSummary),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::modules::ContentEditForbidden),
        (status = 404, response = PageSaveNotFound),
        (status = 409, response = PageSaveConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = PageInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn save_page_body(
    require: Require<CourseContentEdit, PageTarget>,
    State(state): State<AppState>,
    ApiJson(request): ApiJson<PageBodySave>,
) -> Result<Json<PageSummary>, ApiError> {
    let doc = body(&request.body)?;
    let target = require.target();
    let mut tx = begin(&state).await?;
    recheck(&mut tx, &require, target.course.id, PAGE_NOT_FOUND).await?;
    let save = pages::Save {
        body: &doc,
        base_version_id: request.base_version_id,
        author_id: require.principal().user_id,
    };
    let page = pages::save(
        &mut tx,
        require.grants().scope(),
        target.id,
        save,
        state.clock.now(),
    )
    .await
    .map_err(page_error)?;
    commit(tx).await?;
    Ok(Json(PageSummary::from_repo(page)?))
}

/// Delete a page and all its versions, when no item references it.
#[utoipa::path(
    delete,
    path = "/api/v1/pages/{page_id}",
    operation_id = "deletePage",
    tag = "pages",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("page_id" = String, Path, format = "uuid", description = "The page's id.")),
    responses(
        (status = 204, description = "The page is deleted"),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::modules::ContentEditForbidden),
        (status = 404, response = PageNotFound),
        (status = 409, response = PageDeleteConflict),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn delete_page(
    require: Require<CourseContentEdit, PageTarget>,
    State(state): State<AppState>,
) -> Result<StatusCode, ApiError> {
    let target = require.target();
    let mut tx = begin(&state).await?;
    recheck(&mut tx, &require, target.course.id, PAGE_NOT_FOUND).await?;
    pages::delete(&mut tx, require.grants().scope(), target.id)
        .await
        .map_err(page_error)?;
    commit(tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// List the page's versions, newest first, without their bodies.
#[utoipa::path(
    get,
    path = "/api/v1/pages/{page_id}/versions",
    operation_id = "listPageVersions",
    tag = "pages",
    params(
        ("page_id" = String, Path, format = "uuid", description = "The page's id."),
        PageParams,
    ),
    responses(
        (status = 200, description = "One page of the versions", body = Page<PageVersionSummary>),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = PagesViewForbidden),
        (status = 404, response = PageNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_page_versions(
    require: Require<CourseViewAll, PageTarget>,
    State(state): State<AppState>,
    ApiQuery(page): ApiQuery<PageParams>,
) -> Result<Json<Page<PageVersionSummary>>, ApiError> {
    let page = page.parse::<i32>()?;
    let rows = pages::versions(
        &state.db,
        require.grants().scope(),
        require.target().id,
        page.after().copied(),
        page.fetch_limit(),
    )
    .await
    .map_err(page_error)?
    .into_iter()
    .map(PageVersionSummary::from_repo)
    .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(page.page(rows, |row| row.number)?))
}

/// Read one version of the page with its body.
#[utoipa::path(
    get,
    path = "/api/v1/pages/{page_id}/versions/{version_id}",
    operation_id = "getPageVersion",
    tag = "pages",
    params(
        ("page_id" = String, Path, format = "uuid", description = "The page's id."),
        ("version_id" = String, Path, format = "uuid", description = "The version's id."),
    ),
    responses(
        (status = 200, description = "The version with its body", body = PageVersion),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = PagesViewForbidden),
        (status = 404, response = PageVersionNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_page_version(
    require: Require<CourseViewAll, PageTarget>,
    State(state): State<AppState>,
    ApiPath(path): ApiPath<VersionPath>,
) -> Result<Json<PageVersion>, ApiError> {
    let version = pages::version(
        &state.db,
        require.grants().scope(),
        require.target().id,
        path.version_id,
    )
    .await
    .map_err(page_error)?
    .ok_or_else(|| page_error(Error::VersionNotFound))?;
    let summary = version.summary;
    Ok(Json(PageVersion {
        created_at: time(summary.created_at)?,
        id: summary.id,
        number: summary.number,
        author_id: summary.author_id,
        body: version.body,
    }))
}

/// Save a version's body as the page's new current version, with the save's conflict and image
/// checks.
#[utoipa::path(
    post,
    path = "/api/v1/pages/{page_id}/versions/{version_id}/restore",
    operation_id = "restorePageVersion",
    tag = "pages",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(
        ("page_id" = String, Path, format = "uuid", description = "The page's id."),
        ("version_id" = String, Path, format = "uuid", description = "The version to restore."),
    ),
    request_body = PageRestore,
    responses(
        (status = 200, description = "The page with its current version", body = PageSummary),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::modules::ContentEditForbidden),
        (status = 404, response = PageVersionNotFound),
        (status = 409, response = PageSaveConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = PageRestoreInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn restore_page_version(
    require: Require<CourseContentEdit, PageTarget>,
    State(state): State<AppState>,
    ApiPath(path): ApiPath<VersionPath>,
    ApiJson(request): ApiJson<PageRestore>,
) -> Result<Json<PageSummary>, ApiError> {
    let target = require.target();
    let mut tx = begin(&state).await?;
    recheck(&mut tx, &require, target.course.id, PAGE_NOT_FOUND).await?;
    let page = pages::restore(
        &mut tx,
        require.grants().scope(),
        target.id,
        path.version_id,
        (request.base_version_id, require.principal().user_id),
        state.clock.now(),
    )
    .await
    .map_err(page_error)?;
    commit(tx).await?;
    Ok(Json(PageSummary::from_repo(page)?))
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use serde_json::json;

    use super::*;

    #[test]
    fn repository_errors_map_to_their_status_code_and_field() {
        let version = pages::VersionSummary {
            id: Uuid::nil(),
            number: 2,
            author_id: Uuid::max(),
            created_at: UNIX_EPOCH,
        };
        let path = vec!["content".to_owned(), "3".to_owned()];
        let cases = [
            (Error::CourseNotFound, 404, "course.not_found", None),
            (Error::NotFound, 404, "page.not_found", None),
            (Error::FileNotFound, 404, "file.not_found", None),
            (Error::VersionNotFound, 404, "page_version.not_found", None),
            (Error::CourseArchived, 409, "course.archived", None),
            (Error::SlugTaken, 409, "page.slug_taken", None),
            (Error::InUse, 409, "page.in_use", None),
            (
                Error::VersionConflict(version),
                409,
                "page.version_conflict",
                None,
            ),
            (
                Error::SlugUnderivable,
                422,
                "request.invalid",
                Some(("/slug", "page.slug_invalid")),
            ),
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
                Error::TitleContainsNul,
                422,
                "request.invalid",
                Some(("/title", "text.contains_nul")),
            ),
            (
                Error::ImageType { path: path.clone() },
                422,
                "request.invalid",
                Some(("/body/content/3", "page.image_type")),
            ),
            (
                Error::Body(Invalid {
                    path,
                    reason: "test",
                }),
                422,
                "request.invalid",
                Some(("/body/content/3", "page.body_invalid")),
            ),
        ];
        for (error, status, code, field) in cases {
            let problem = page_error(error).problem();
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
    fn a_conflict_names_the_current_version() {
        let version = pages::VersionSummary {
            id: Uuid::nil(),
            number: 2,
            author_id: Uuid::max(),
            created_at: UNIX_EPOCH,
        };
        let json = serde_json::to_value(page_error(Error::VersionConflict(version)).problem())
            .expect("a problem serializes");
        assert_eq!(
            json["current_version"],
            json!({ "id": Uuid::nil(), "number": 2, "author_id": Uuid::max() })
        );
    }

    #[test]
    fn requests_refuse_null_and_unknown_members() {
        for refused in [
            json!({ "title": null }),
            json!({ "slug": null }),
            json!({ "course_id": "x" }),
        ] {
            assert!(
                serde_json::from_value::<PagePatch>(refused.clone()).is_err(),
                "{refused}"
            );
        }
        assert!(serde_json::from_value::<NewPage>(json!({ "title": "T", "slug": null })).is_err());
        assert!(
            serde_json::from_value::<PageRestore>(
                json!({ "base_version_id": Uuid::nil(), "x": 1 })
            )
            .is_err()
        );
    }

    #[test]
    fn a_slug_is_checked_as_given() {
        let code = |e: ApiError| e.problem().errors[0].clone();
        assert!(slug("ground-control-101").is_ok());
        for bad in ["Ground", "a--b", "-a", ""] {
            assert_eq!(
                code(slug(bad).unwrap_err()),
                FieldError::at(&["slug"], "page.slug_invalid"),
                "{bad}"
            );
        }
    }
}
