// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reading a page (docs/specs/m1/07-pages-and-the-editor.md B14 to B18): `GET
//! /api/v1/pages/{page_id}`, the current version rendered to sanitized HTML for whoever may read
//! it, and its body JSON for the editor.
//!
//! The route takes `Require<perm::CourseView, PageTarget>`, then decides in one `REPEATABLE READ`
//! snapshot, after checking the course again (`outline::view`): a holder of `course.view_all`
//! reads every page; anyone else reads the course's home page, or a page a published item of a
//! published module references and that item is unlocked for them (spec 04, with their own
//! completed items when they are a learner and none otherwise). A page referenced only by items
//! locked for them answers 403 `item.locked` with the reasons of the first such item in course
//! order; any other page 404 `page.not_found`, as one that does not exist. `?format=json`
//! answers the body and needs `course.view_all`.
//!
//! The HTML comes from the render cache (`pages::rendered`) with its images' placeholders
//! filled with signed inline URLs issued for this request (spec 06 B16), so the cache never
//! holds a credential and every reader gets URLs of their own.

use std::collections::{HashMap, HashSet};

use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use stepclimb_core::{
    OrgScope, course_files, courses,
    grants::Denied,
    id::Uuid,
    page_doc::{self, PageDoc},
    pages::{self, Error},
    perm::CourseView,
    progress, unlock,
};
use utoipa::{IntoParams, ToSchema};

use crate::{
    AppState,
    error::ApiError,
    extract::ApiQuery,
    file_urls::{self, Disposition},
    openapi::problem_responses,
    outline::{self, Viewer},
    pages::page_error,
    progress::progress_error,
    require::{PageTarget, Require, code::PAGE_NOT_FOUND},
    timestamp,
    unlock::item_locked,
};

problem_responses! {
    PageReadNotFound => "`page.not_found`: no page with this id exists in the organization, or you may not read it: its course is out of your reach, a draft or a template, or, without `course.view_all`, it is neither the course's home page nor referenced by a published item of a published module. `org.not_found`: the request's organization cannot be determined.",
    PageReadForbidden => "`permission.denied`: you do not hold `course.view` at a scope containing the course, or you asked for `format=json` without `course.view_all`. `item.locked`: every item that references the page is locked for you; `reasons` says what the first one waits for.",
}

/// How a page is answered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PageFormat {
    /// The current version rendered to sanitized HTML.
    #[default]
    Html,
    /// The current version's body JSON, for the editor; needs `course.view_all`.
    Json,
}

/// The query of a page read.
#[derive(Clone, Copy, Debug, Default, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct PageReadQuery {
    /// `html` when omitted.
    pub format: Option<PageFormat>,
}

/// The version a page read answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct PageViewVersion {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub number: i32,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
}

/// A page as a reader gets it: its current version as HTML, or as body JSON for the editor.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct PageView {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub course_id: Uuid,
    pub title: String,
    pub slug: String,
    pub version: PageViewVersion,
    /// The current version rendered and sanitized by the server, without `format=json`. Images
    /// carry signed URLs that expire 10 minutes after the read; read the page again for new
    /// ones. Insert it as given; attach no handler from it.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false, nullable = false)]
    pub html: Option<String>,
    /// The current version's body, with `format=json`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<crate::page_doc_schema::PageDoc>, required = false, nullable = false)]
    pub body: Option<Value>,
}

/// A page of a course, as whoever may read it gets it.
#[utoipa::path(
    get,
    path = "/api/v1/pages/{page_id}",
    operation_id = "getPage",
    tag = "pages",
    params(
        ("page_id" = String, Path, format = "uuid", description = "The page's id."),
        PageReadQuery,
    ),
    responses(
        (status = 200, description = "The page's current version", body = PageView),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = PageReadForbidden),
        (status = 404, response = PageReadNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_page(
    require: Require<CourseView, PageTarget>,
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<PageReadQuery>,
) -> Result<Json<PageView>, ApiError> {
    let target = require.target();
    let (scope, page_id, course_id) = (require.grants().scope(), target.id, target.course.id);
    let format = query.format.unwrap_or_default();
    let mut tx = outline::begin_read(&state).await?;
    let viewer = outline::view(&mut tx, &require, course_id, PAGE_NOT_FOUND).await?;
    if !viewer.sees_all {
        may_read(&mut tx, &require, viewer, (course_id, page_id), &state).await?;
        // After the reach check, so a page the caller may not read answers 404 in any format.
        if format == PageFormat::Json {
            return Err(Denied::Forbidden.into());
        }
    }
    let page = pages::get(&mut *tx, scope, page_id)
        .await
        .map_err(page_error)?
        .ok_or_else(not_found)?;
    let version = pages::version(&mut *tx, scope, page_id, page.current.id)
        .await
        .map_err(page_error)?
        .ok_or_else(not_found)?;
    outline::end(tx).await?;
    let summary = &version.summary;
    let mut view = PageView {
        id: page.id,
        course_id: page.course_id,
        title: page.title,
        slug: page.slug,
        version: PageViewVersion {
            id: summary.id,
            number: summary.number,
            created_at: timestamp::rfc3339(summary.created_at)
                .map_err(|error| ApiError::internal(&error))?,
        },
        html: None,
        body: None,
    };
    match format {
        PageFormat::Json => view.body = Some(version.body),
        PageFormat::Html => view.html = Some(render(&state, scope, &version).await?),
    }
    Ok(Json(view))
}

/// `version` as HTML with signed URLs for its images (B15).
pub(crate) async fn render(
    state: &AppState,
    scope: OrgScope,
    version: &pages::Version,
) -> Result<String, ApiError> {
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(|error| ApiError::internal(&error))?;
    let html = pages::rendered(&mut conn, scope, version)
        .await
        .map_err(page_error)?;
    let doc = PageDoc::parse_stored(&version.body).map_err(|error| ApiError::internal(&error))?;
    let ids: HashSet<Uuid> = doc.images().into_iter().map(|(id, _)| id).collect();
    let mut urls = HashMap::new();
    for id in ids {
        // A version's images are kept by `page_version_files`, so each course file exists.
        let file = course_files::get(&mut *conn, scope, id)
            .await
            .map_err(|error| ApiError::internal(&error))?
            .ok_or_else(not_found)?;
        let signed = file_urls::signed(state, scope, &file, Disposition::Inline)?;
        urls.insert(id, signed.url);
    }
    Ok(page_doc::fill_images(&html, &urls))
}

/// Whether a caller without `course.view_all` may read the page `page_id` of `course_id` (B18):
/// it is the course's home page, or an item that references it is unlocked for them. Otherwise
/// 403 `item.locked` with the first locked item's reasons, or 404.
async fn may_read(
    tx: &mut Transaction<'static, Postgres>,
    require: &Require<CourseView, PageTarget>,
    viewer: Viewer,
    (course_id, page_id): (Uuid, Uuid),
    state: &AppState,
) -> Result<(), ApiError> {
    let scope = require.grants().scope();
    let course = courses::get(&mut **tx, scope, course_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(not_found)?;
    if course.settings.home == (courses::Home::Page { page_id }) {
        return Ok(());
    }
    let items = pages::items_referencing(&mut **tx, scope, page_id)
        .await
        .map_err(page_error)?;
    if items.is_empty() {
        return Err(not_found());
    }
    let completed = if viewer.learner {
        progress::completed_items(&mut **tx, scope, require.principal().user_id, course_id)
            .await
            .map_err(progress_error)?
    } else {
        HashSet::new()
    };
    let rules = unlock::load_course_rules(&mut **tx, scope, course_id)
        .await
        .map_err(crate::modules::content_error)?;
    let evaluation = unlock::evaluate(&rules, &completed, state.clock.now());
    let mut first_locked = None;
    for item in items {
        match evaluation.item(item).map(|item| &item.state) {
            Some(unlock::State::Available) => return Ok(()),
            Some(unlock::State::Locked(reasons)) => {
                first_locked.get_or_insert_with(|| reasons.clone());
            }
            // Unpublished, or in an unpublished module: the reader cannot see it.
            None => {}
        }
    }
    Err(first_locked.map_or_else(not_found, |reasons| item_locked(&reasons)))
}

fn not_found() -> ApiError {
    page_error(Error::NotFound)
}
