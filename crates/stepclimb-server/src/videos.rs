// SPDX-License-Identifier: AGPL-3.0-or-later
//! A course's videos (docs/specs/m1/08-uploaded-video.md B1, B2 and API): list the videos
//! attached to a course, read a video and rename it. Uploading is [`crate::video_uploads`]; this
//! module holds the `Video` schema and the error mapping both use.
//!
//! The reads take `Require<perm::CourseViewAll, _>`, at the course or through the video
//! ([`VideoTarget`], which passes when the permission holds at any course the video is attached
//! to). The rename takes `Require<perm::CourseContentEdit, VideoTarget>` and checks it again
//! under the lock of each of those courses, as [`crate::modules`] does. The retry (B2) takes the
//! same permission and checks, moves a video that failed as `video.transcode_failed` back to
//! `processing` and enqueues its `video.transcode` job in the same transaction. None of these
//! routes reads stored bytes, so none needs storage.

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use stepclimb_core::{
    courses,
    grants::Denied,
    id::Uuid,
    perm::{CourseContentEdit, CourseViewAll},
    transcodes::{self, Retried},
    videos::{self, Error},
};
use stepclimb_jobs::{NewJob, enqueue};
use utoipa::ToSchema;

use crate::{
    AppState,
    error::{ApiError, FieldError},
    extract::{ApiJson, ApiQuery},
    modules::{begin, commit, recheck},
    openapi::problem_responses,
    organization::present,
    pagination::{Page, PageParams},
    require::{
        CourseTarget, Require, VideoTarget,
        code::{COURSE_NOT_FOUND, VIDEO_NOT_FOUND, VIDEO_UPLOAD_NOT_FOUND},
        not_found_as,
    },
    roles::code::{NAME_EMPTY, NAME_TOO_LONG, TEXT_CONTAINS_NUL},
    timestamp,
    video_transcode::TRANSCODE,
};

/// The codes this module and [`crate::video_uploads`] produce besides the targets' 404s.
pub mod code {
    /// A chunk's `Upload-Offset` is not the upload's stored offset (409, B5).
    pub const UPLOAD_OFFSET_MISMATCH: &str = "video.upload_offset_mismatch";
    /// The video is past the status the operation needs (409).
    pub const STATUS_INVALID: &str = "video.status_invalid";
}

problem_responses! {
    VideoNotFound => "`video.not_found`: no video with this id exists in the organization, or you cannot see any course it is attached to. `org.not_found`: the request's organization cannot be determined.",
    VideosCourseNotFound => "`course.not_found`: no course with this id exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined.",
    VideosViewForbidden => "`permission.denied`: you can see the course, or a course the video is attached to, but do not hold `course.view_all` at a scope containing it.",
    VideoEditForbidden => "`permission.denied`: you can see a course the video is attached to but do not hold `course.content_edit` at a scope containing any of them. `auth.csrf_refused`: the request did not come from this site.",
    VideoRenameConflict => "`course.archived`: every course through which you may edit the video is archived.",
    VideoRetryConflict => "`video.status_invalid`: the video did not fail as `video.transcode_failed`, or its upload is no longer stored; upload it again as a new video. `course.archived`: every course through which you may edit the video is archived.",
    VideoInvalid => "`request.invalid`: `errors` names `/title` with `name.empty` (empty after trimming), `name.too_long` (over 200 characters) or `text.contains_nul`.",
}

/// A video of the organization.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Video {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    /// `upload` for an uploaded video.
    pub source: String,
    pub title: String,
    /// `uploading`, `processing`, `ready` or `failed`.
    pub status: String,
    /// Why the video failed (`video.not_a_video`, ...); only when `status` is `failed`.
    pub error_code: Option<String>,
    /// The length in seconds, once a transcode has read it.
    pub duration_s: Option<f64>,
    /// What a successful transcode produced; absent until then.
    pub renditions: Option<Renditions>,
    /// By `at_s`.
    pub chapters: Vec<Chapter>,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub updated_at: String,
}

/// The HLS renditions of a ready video.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Renditions {
    /// Their heights in pixels, shortest first (360, 720, 1080, or one below 360).
    pub heights: Vec<u32>,
}

/// A chapter mark of a video.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Chapter {
    /// Seconds from the start.
    pub at_s: f64,
    pub title: String,
}

impl Video {
    pub(crate) fn from_repo(video: videos::Video) -> Result<Self, ApiError> {
        let time = |t| timestamp::rfc3339(t).map_err(|error| ApiError::internal(&error));
        Ok(Self {
            created_at: time(video.created_at)?,
            updated_at: time(video.updated_at)?,
            id: video.id,
            source: video.source,
            title: video.title,
            status: video.status.as_str().to_owned(),
            error_code: video.error_code,
            duration_s: video.duration_s,
            renditions: (!video.heights.is_empty()).then_some(Renditions {
                heights: video.heights,
            }),
            chapters: video
                .chapters
                .into_iter()
                .map(|c| Chapter {
                    at_s: c.at_s,
                    title: c.title,
                })
                .collect(),
        })
    }
}

/// A change to a video; an omitted member is left as it is.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct VideoPatch {
    /// 1 to 200 characters after trimming.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub title: Option<String>,
}

/// The answer for a videos repository error.
pub(crate) fn video_error(error: Error) -> ApiError {
    let invalid = |code| ApiError::invalid(vec![FieldError::at(&["title"], code)]);
    match error {
        Error::CourseNotFound => not_found_as(Denied::CourseNotFound, COURSE_NOT_FOUND),
        Error::UploadNotFound => not_found_as(Denied::CourseNotFound, VIDEO_UPLOAD_NOT_FOUND),
        Error::CourseArchived => ApiError::new(
            StatusCode::CONFLICT,
            crate::modules::code::COURSE_ARCHIVED,
            "The course is archived and takes no content change.",
        ),
        Error::OffsetMismatch => ApiError::new(
            StatusCode::CONFLICT,
            code::UPLOAD_OFFSET_MISMATCH,
            "Upload-Offset is not the upload's offset; ask for it with HEAD and resume from there.",
        ),
        Error::TitleEmpty => invalid(NAME_EMPTY),
        Error::TitleTooLong => invalid(NAME_TOO_LONG),
        Error::ContainsNul { .. } => invalid(TEXT_CONTAINS_NUL),
        Error::LengthInvalid => ApiError::malformed("Upload-Length is not a number of bytes."),
        other @ (Error::Stored { .. } | Error::Time(_) | Error::Database(_)) => {
            ApiError::internal(&other)
        }
    }
}

/// List the videos attached to the course, oldest first.
#[utoipa::path(
    get,
    path = "/api/v1/courses/{course_id}/videos",
    operation_id = "listCourseVideos",
    tag = "videos",
    params(
        ("course_id" = String, Path, format = "uuid", description = "The course's id."),
        PageParams,
    ),
    responses(
        (status = 200, description = "One page of the course's videos", body = Page<Video>),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = VideosViewForbidden),
        (status = 404, response = VideosCourseNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_course_videos(
    require: Require<CourseViewAll, CourseTarget>,
    State(state): State<AppState>,
    ApiQuery(page): ApiQuery<PageParams>,
) -> Result<Json<Page<Video>>, ApiError> {
    let page = page.parse::<Uuid>()?;
    let scope = require.grants().scope();
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(|error| ApiError::internal(&error))?;
    let rows = videos::list_for_course(
        &mut conn,
        scope,
        require.target().id,
        page.after().copied(),
        page.fetch_limit(),
    )
    .await
    .map_err(video_error)?;
    let rows = rows
        .into_iter()
        .map(Video::from_repo)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(page.page(rows, |video| video.id)?))
}

/// Read a video.
#[utoipa::path(
    get,
    path = "/api/v1/videos/{video_id}",
    operation_id = "getVideo",
    tag = "videos",
    params(("video_id" = String, Path, format = "uuid", description = "The video's id.")),
    responses(
        (status = 200, description = "The video", body = Video),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = VideosViewForbidden),
        (status = 404, response = VideoNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_video(
    require: Require<CourseViewAll, VideoTarget>,
    State(state): State<AppState>,
) -> Result<Json<Video>, ApiError> {
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(|error| ApiError::internal(&error))?;
    let video = videos::get(&mut conn, require.grants().scope(), require.target().id)
        .await
        .map_err(video_error)?
        .ok_or_else(|| not_found_as(Denied::CourseNotFound, VIDEO_NOT_FOUND))?;
    Ok(Json(Video::from_repo(video)?))
}

/// Rename a video. A patch with no member changes nothing and answers the video.
#[utoipa::path(
    patch,
    path = "/api/v1/videos/{video_id}",
    operation_id = "updateVideo",
    tag = "videos",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("video_id" = String, Path, format = "uuid", description = "The video's id.")),
    request_body = VideoPatch,
    responses(
        (status = 200, description = "The video after the change", body = Video),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = VideoEditForbidden),
        (status = 404, response = VideoNotFound),
        (status = 409, response = VideoRenameConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = VideoInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_video(
    require: Require<CourseContentEdit, VideoTarget>,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<VideoPatch>,
) -> Result<Json<Video>, ApiError> {
    if let Some(title) = &body.title {
        videos::title(title).map_err(video_error)?;
    }
    let (scope, target) = (require.grants().scope(), require.target());
    let mut tx = begin(&state).await?;
    let mut open = Vec::new();
    for course in &target.courses {
        let locked = recheck(&mut tx, &require, course.id, VIDEO_NOT_FOUND).await?;
        if locked.status != courses::Status::Archived {
            open.push(course.id);
        }
    }
    let hidden = || not_found_as(Denied::CourseNotFound, VIDEO_NOT_FOUND);
    let video = match &body.title {
        None => videos::get(&mut tx, scope, target.id).await,
        Some(_) if open.is_empty() => return Err(video_error(Error::CourseArchived)),
        Some(title) => videos::rename(&mut tx, scope, target.id, (&open, title), now(&state)).await,
    }
    .map_err(video_error)?
    .ok_or_else(hidden)?;
    commit(tx).await?;
    Ok(Json(Video::from_repo(video)?))
}

/// Transcode again a video whose transcode failed (`video.transcode_failed`), from its upload,
/// which the server keeps until a transcode succeeds. The video is `processing` again when this
/// answers; read it to follow the new transcode.
#[utoipa::path(
    post,
    path = "/api/v1/videos/{video_id}/retry",
    operation_id = "retryVideo",
    tag = "videos",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("video_id" = String, Path, format = "uuid", description = "The video's id.")),
    responses(
        (status = 202, description = "The video, processing again", body = Video),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = VideoEditForbidden),
        (status = 404, response = VideoNotFound),
        (status = 409, response = VideoRetryConflict),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn retry_video(
    require: Require<CourseContentEdit, VideoTarget>,
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<Video>), ApiError> {
    let (scope, target) = (require.grants().scope(), require.target());
    let mut tx = begin(&state).await?;
    let mut open = Vec::new();
    for course in &target.courses {
        let locked = recheck(&mut tx, &require, course.id, VIDEO_NOT_FOUND).await?;
        if locked.status != courses::Status::Archived {
            open.push(course.id);
        }
    }
    if open.is_empty() {
        return Err(video_error(Error::CourseArchived));
    }
    let hidden = || not_found_as(Denied::CourseNotFound, VIDEO_NOT_FOUND);
    let retried = transcodes::retry(&mut tx, scope, (target.id, &open), now(&state))
        .await
        .map_err(video_error)?;
    match retried {
        Retried::Processing => {}
        Retried::NotFound => return Err(hidden()),
        Retried::StatusInvalid => {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                code::STATUS_INVALID,
                "Only a video whose transcode failed, with its upload still stored, can be retried.",
            ));
        }
    }
    let key = target.id.to_string();
    let payload = serde_json::json!({ "video_id": target.id });
    let job = NewJob::org(&TRANSCODE, scope, &payload)
        .and_then(|job| job.dedupe(&key))
        .map_err(|error| ApiError::internal(&error))?;
    enqueue(&mut tx, job, now(&state))
        .await
        .map_err(|error| ApiError::internal(&error))?;
    let video = videos::get(&mut tx, scope, target.id)
        .await
        .map_err(video_error)?
        .ok_or_else(hidden)?;
    commit(tx).await?;
    Ok((StatusCode::ACCEPTED, Json(Video::from_repo(video)?)))
}

fn now(state: &AppState) -> std::time::SystemTime {
    state.clock.now()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn repository_errors_map_to_their_status_code_and_field() {
        let cases = [
            (Error::CourseNotFound, 404, "course.not_found", None),
            (Error::UploadNotFound, 404, "video_upload.not_found", None),
            (Error::CourseArchived, 409, "course.archived", None),
            (
                Error::OffsetMismatch,
                409,
                "video.upload_offset_mismatch",
                None,
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
            (Error::LengthInvalid, 400, "request.malformed", None),
        ];
        for (error, status, code, field) in cases {
            let problem = video_error(error).problem();
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
    fn a_patch_refuses_null_and_unknown_members() {
        for refused in [json!({ "title": null }), json!({ "status": "ready" })] {
            assert!(
                serde_json::from_value::<VideoPatch>(refused.clone()).is_err(),
                "{refused}"
            );
        }
    }
}
