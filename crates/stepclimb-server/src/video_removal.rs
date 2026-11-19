// SPDX-License-Identifier: AGPL-3.0-or-later
//! Deleting a video and detaching it from a course (docs/specs/m1/08-uploaded-video.md B20, D46,
//! D47), and `videos.purge`, the job that deletes a deleted video's stored objects.
//!
//! `DELETE /api/v1/videos/{video_id}` takes `Require<perm::CourseContentEdit, VideoTarget>` and
//! needs the permission at every course the video is attached to: under the lock of each, it
//! checks the permission again and that none is archived, then compares the courses with those
//! the video is attached to now, answering 403 when the caller may not edit one of them.
//! `DELETE /api/v1/courses/{course_id}/videos/{video_id}` takes `Require<perm::CourseContentEdit,
//! CourseTarget>` and the course's lock. Both then call `stepclimb_core::video_removal`, which
//! refuses a video an item names (409 `video.in_use`) and, when the video itself would go, one
//! being transcoded (409 `video.status_invalid`).
//!
//! A deleted video's objects (`orgs/<org>/videos/<video>/`: its renditions and any upload parts)
//! are deleted after the commit by `videos.purge`, enqueued in the same transaction. The job
//! deletes nothing while a video with that id exists, and retries with the queue's backoff when
//! the store fails, so a delete that fails is not lost. Neither route reads stored bytes; without
//! storage no video could have been uploaded, and the job has nothing to delete.

use std::time::Duration;

use axum::{extract::State, http::StatusCode};
use stepclimb_core::{
    OrgScope, courses,
    grants::Denied,
    id::Uuid,
    perm::CourseContentEdit,
    video_removal::{self, Detached, Error},
    videos,
};
use stepclimb_jobs::{JobContext, Kind, NewJob, Outcome, Registry, RegistryError, Retry, enqueue};

use crate::{
    AppState,
    error::ApiError,
    extract::ApiPath,
    modules::{begin, commit, recheck},
    openapi::problem_responses,
    require::{
        CourseTarget, Require, VideoTarget,
        code::{COURSE_NOT_FOUND, VIDEO_NOT_FOUND},
        not_found_as,
    },
    storage::Storage,
    video_parts,
    videos::{code::STATUS_INVALID, video_error},
};

/// The job that deletes a deleted video's objects.
pub const PURGE: Kind = Kind::new("videos.purge", "default").with_retry(Retry {
    base: Duration::from_secs(60),
    cap: Duration::from_secs(60 * 60),
    max_attempts: 10,
});

/// The codes this module produces besides the targets' 404s.
pub mod code {
    /// An item still names the video (409).
    pub const IN_USE: &str = "video.in_use";
}

problem_responses! {
    VideoDeleteForbidden => "`permission.denied`: you do not hold `course.content_edit` at a scope containing every course the video is attached to. `auth.csrf_refused`: the request did not come from this site.",
    VideoDeleteConflict => "`video.in_use`: an item of a course names the video; delete the item first. `video.status_invalid`: the video is being transcoded; wait until it is ready or failed. `course.archived`: a course the video is attached to is archived.",
    VideoDetachForbidden => "`permission.denied`: you can see the course but do not hold `course.content_edit` at a scope containing it. `auth.csrf_refused`: the request did not come from this site.",
    VideoDetachNotFound => "`course.not_found`: no course with this id exists in the organization, or you cannot see it. `video.not_found`: no video with this id is attached to the course. `org.not_found`: the request's organization cannot be determined.",
    VideoDetachConflict => "`video.in_use`: an item of the course names the video; delete the item first. `video.status_invalid`: the course is the video's last and the video is being transcoded; wait until it is ready or failed. `course.archived`: the course is archived.",
}

/// Delete a video that no item of any course names, with its renditions and every attachment.
#[utoipa::path(
    delete,
    path = "/api/v1/videos/{video_id}",
    operation_id = "deleteVideo",
    tag = "videos",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("video_id" = String, Path, format = "uuid", description = "The video's id.")),
    responses(
        (status = 204, description = "The video is deleted; its stored objects are deleted shortly after"),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = VideoDeleteForbidden),
        (status = 404, response = crate::videos::VideoNotFound),
        (status = 409, response = VideoDeleteConflict),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn delete_video(
    require: Require<CourseContentEdit, VideoTarget>,
    State(state): State<AppState>,
) -> Result<StatusCode, ApiError> {
    let (scope, target) = (require.grants().scope(), require.target());
    let mut tx = begin(&state).await?;
    let mut editable = Vec::new();
    for course in &target.courses {
        let locked = recheck(&mut tx, &require, course.id, VIDEO_NOT_FOUND).await?;
        if locked.status == courses::Status::Archived {
            return Err(video_error(videos::Error::CourseArchived));
        }
        editable.push(course.id);
    }
    // The video's courses now, under the locks: only a new video gains a course, so the list
    // cannot grow past the locked set; a course outside it is one the caller may not edit.
    let attached = videos::courses_of(&mut *tx, scope, target.id)
        .await
        .map_err(video_error)?;
    if attached.is_empty() {
        return Err(not_found_as(Denied::CourseNotFound, VIDEO_NOT_FOUND));
    }
    if attached.iter().any(|course| !editable.contains(course)) {
        return Err(not_found_as(Denied::Forbidden, VIDEO_NOT_FOUND));
    }
    video_removal::delete(&mut tx, scope, target.id)
        .await
        .map_err(removal_error)?;
    purge_later(&mut tx, &state, scope, target.id).await?;
    commit(tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Detach a video from a course when no item of the course names it. Detaching it from its last
/// course deletes the video, as `deleteVideo` does.
#[utoipa::path(
    delete,
    path = "/api/v1/courses/{course_id}/videos/{video_id}",
    operation_id = "detachCourseVideo",
    tag = "videos",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(
        ("course_id" = String, Path, format = "uuid", description = "The course's id."),
        ("video_id" = String, Path, format = "uuid", description = "The video's id."),
    ),
    responses(
        (status = 204, description = "The video is no longer attached to the course; when it was its last, the video is deleted and its stored objects are deleted shortly after"),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = VideoDetachForbidden),
        (status = 404, response = VideoDetachNotFound),
        (status = 409, response = VideoDetachConflict),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn detach_course_video(
    require: Require<CourseContentEdit, CourseTarget>,
    State(state): State<AppState>,
    ApiPath((_, video_id)): ApiPath<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let (scope, course_id) = (require.grants().scope(), require.target().id);
    let mut tx = begin(&state).await?;
    let locked = recheck(&mut tx, &require, course_id, COURSE_NOT_FOUND).await?;
    if locked.status == courses::Status::Archived {
        return Err(video_error(videos::Error::CourseArchived));
    }
    let detached = video_removal::detach(&mut tx, scope, course_id, video_id)
        .await
        .map_err(removal_error)?;
    if detached == Detached::Video {
        purge_later(&mut tx, &state, scope, video_id).await?;
    }
    commit(tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Enqueue `videos.purge` for the video `video_id` in the transaction that deletes it.
async fn purge_later(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    state: &AppState,
    scope: OrgScope,
    video_id: Uuid,
) -> Result<(), ApiError> {
    let key = video_id.to_string();
    let payload = serde_json::json!({ "video_id": video_id });
    let job = NewJob::org(&PURGE, scope, &payload)
        .and_then(|job| job.dedupe(&key))
        .map_err(|error| ApiError::internal(&error))?;
    enqueue(tx, job, state.clock.now())
        .await
        .map_err(|error| ApiError::internal(&error))?;
    Ok(())
}

/// The answer for a removal error.
fn removal_error(error: Error) -> ApiError {
    match error {
        Error::VideoNotFound => not_found_as(Denied::CourseNotFound, VIDEO_NOT_FOUND),
        Error::InUse => ApiError::new(
            StatusCode::CONFLICT,
            code::IN_USE,
            "An item still uses this video; delete the item first.",
        ),
        Error::Processing => ApiError::new(
            StatusCode::CONFLICT,
            STATUS_INVALID,
            "The video is being transcoded; wait until it is ready or failed.",
        ),
        Error::Database(error) => ApiError::internal(&error),
    }
}

/// Register `videos.purge` in `registry`, with the process's `storage`.
pub fn register(registry: &mut Registry, storage: Option<Storage>) -> Result<(), RegistryError> {
    registry.register(PURGE, move |ctx: JobContext| {
        let storage = storage.clone();
        async move { purge(ctx, storage).await }
    })
}

/// Delete every object of the deleted video the payload names; nothing while a video with that id
/// exists.
async fn purge(ctx: JobContext, storage: Option<Storage>) -> Outcome {
    let Some(scope) = ctx.scope() else {
        return Outcome::fail("videos.purge is an organization job");
    };
    let Some(video_id) = ctx.payload()["video_id"]
        .as_str()
        .and_then(|v| v.parse::<Uuid>().ok())
    else {
        return Outcome::fail("the payload names no video_id");
    };
    let Some(storage) = storage else {
        return Outcome::Done;
    };
    match video_removal::exists(ctx.pool(), scope, video_id).await {
        Ok(false) => {}
        Ok(true) => return Outcome::Done,
        Err(error) => return Outcome::retry(format_args!("could not read the video: {error}")),
    }
    match video_parts::delete_tree(&storage, &storage.video_prefix(scope, video_id)).await {
        Ok(_) => Outcome::Done,
        Err(error) => Outcome::retry(format_args!(
            "could not delete the video's objects: {error}"
        )),
    }
}
