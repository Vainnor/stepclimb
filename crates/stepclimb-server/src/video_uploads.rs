// SPDX-License-Identifier: AGPL-3.0-or-later
//! Uploading a video with tus 1.0.0: the core protocol with the creation and termination
//! extensions (docs/specs/m1/08-uploaded-video.md B4 to B7, Q1).
//!
//! - `POST /api/v1/courses/{course_id}/video-uploads` (creation) checks, before creating
//!   anything, that storage is configured (503), that `Upload-Length` is at most
//!   `max_video_bytes` (413), that the course is not archived (409) and that the length fits the
//!   plan's `video_storage` limit (403), the last two under the course lock. It then creates the
//!   `videos` row (`uploading`), attached to the course, and the `video_uploads` row in that
//!   transaction.
//! - `HEAD /api/v1/video-uploads/{upload_id}` answers the stored `Upload-Offset` and
//!   `Upload-Length`.
//! - `PATCH` streams one chunk, at most [`MAX_CHUNK_BYTES`] and at least [`MIN_CHUNK_BYTES`]
//!   unless it completes the upload, to a temporary object through
//!   [`crate::upload::stream_to_temporary`], so no chunk is held whole in memory. Then, in one
//!   transaction, it checks the permission again with the course row `FOR SHARE` (an archive
//!   waits for it; chunks of other uploads do not), locks the upload row, answers 404 for an
//!   upload being deleted, refuses an `Upload-Offset` that is no longer the stored one (409),
//!   copies the temporary
//!   object to `orgs/<org>/videos/<video>/upload/<offset>` and adds the chunk's length to the
//!   row in one statement. The row lock makes chunks of one upload take turns, so any server of
//!   the instance can take the next chunk and two chunks at one offset never both count. When
//!   the offset reaches the length, the same transaction sniffs its first 8 KiB
//!   (`stepclimb_core::sniff::video`): a video becomes `processing` with a `video.transcode` job
//!   on the `transcode` queue, anything else `failed` with `video.not_a_video` and its upload
//!   marked terminating, whose parts [`crate::video_parts::finish`] deletes after the commit.
//! - `DELETE` (termination) marks the upload terminating under its row's lock, commits, then
//!   deletes the parts and the video holding no lock ([`crate::video_parts`]).
//!
//! Every request needs `Tus-Resumable: 1.0.0` (412 `tus.version_unsupported` otherwise, with
//! `Tus-Version`), and every response of these routes carries `Tus-Resumable` ([`tus_header`]).
//! Each route needs `perm::CourseContentEdit` at the course; the unsafe ones pass the CSRF check
//! like every other route.
//!
//! An object written at an offset is overwritten by the next chunk committed at that offset, so
//! a chunk whose commit failed leaves at most one part at `received`, which the next chunk at
//! that offset replaces: the parts below `received` are the upload's bytes.

use axum::{
    Json,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use object_store::{ObjectStoreExt as _, path::Path};
use serde::Serialize;
use sqlx::{Postgres, Transaction};
use stepclimb_core::{
    OrgScope, courses,
    feature::Limit,
    perm::CourseContentEdit,
    sniff,
    videos::{self, NewUpload, Terminating, Upload},
};
use stepclimb_jobs::{Kind, NewJob, enqueue};

use crate::{
    AppState,
    entitlements::Entitlements,
    error::{ApiError, FieldError, code::REQUEST_UNSUPPORTED_MEDIA_TYPE},
    modules::{begin, commit, recheck, recheck_shared},
    openapi::problem_responses,
    require::{
        CourseTarget, Require, VideoUploadTarget,
        code::{COURSE_NOT_FOUND, VIDEO_UPLOAD_NOT_FOUND},
    },
    roles::code::NAME_EMPTY,
    storage::Storage,
    upload::{self, content_length, stream_to_temporary},
    video_parts,
    videos::{Video, code as video_code, video_error},
};

/// The tus protocol version these routes speak.
pub const TUS_VERSION: &str = "1.0.0";

/// The largest chunk one `PATCH` may send: 64 MiB (B6).
pub const MAX_CHUNK_BYTES: u64 = 64 * 1024 * 1024;

/// The smallest chunk one `PATCH` may send, unless it is the one that completes the upload: 1 MiB
/// (D15), so a 50 GiB upload has at most 51,200 parts to store and delete.
pub const MIN_CHUNK_BYTES: u64 = 1024 * 1024;

/// The path of an upload; `{VIDEO_UPLOADS_PATH}/{upload_id}`.
pub const VIDEO_UPLOADS_PATH: &str = "/api/v1/video-uploads";

/// The job that transcodes a video once its upload is complete (B7, B9).
pub const TRANSCODE: Kind = Kind::new("video.transcode", "transcode");

/// The codes this module produces besides [`crate::videos::code`]'s.
pub mod code {
    /// `Tus-Resumable` is missing or names a version other than 1.0.0 (412).
    pub const TUS_VERSION_UNSUPPORTED: &str = "tus.version_unsupported";
    /// A chunk under [`super::MIN_CHUNK_BYTES`] that does not complete the upload (400).
    pub const CHUNK_TOO_SMALL: &str = "video.chunk_too_small";
}

const TUS_RESUMABLE: &str = "tus-resumable";
const TUS_VERSION_HEADER: &str = "tus-version";
const UPLOAD_LENGTH: &str = "upload-length";
const UPLOAD_OFFSET: &str = "upload-offset";
const UPLOAD_METADATA: &str = "upload-metadata";
const OFFSET_OCTET_STREAM: &str = "application/offset+octet-stream";

problem_responses! {
    VideoUploadCourseNotFound => "`course.not_found`: no course with this id exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined.",
    VideoUploadNotFound => "`video_upload.not_found`: no video upload with this id exists in the organization, it is being deleted, or you cannot see its course. `org.not_found`: the request's organization cannot be determined.",
    VideoUploadForbidden => "`permission.denied`: you can see the course but do not hold `course.content_edit` at a scope containing it. `entitlement.limit_reached`: the upload's length would take the organization past its plan's `video_storage` limit; `limit` and `max` say which and how much. `auth.csrf_refused`: the request did not come from this site.",
    VideoUploadEditForbidden => "`permission.denied`: you can see the upload's course but do not hold `course.content_edit` at a scope containing it. `auth.csrf_refused`: the request did not come from this site.",
    VideoUploadCreateBadRequest => "`request.malformed`: `Upload-Length` is missing, not a number or 0, or `Upload-Metadata` is not comma-separated `key base64value` pairs with each key once and UTF-8 values.",
    VideoUploadCreateConflict => "`course.archived`: the course is archived and takes no content change.",
    VideoUploadCreateTooLarge => "`file.too_large`: `Upload-Length` is over the largest video this server accepts (`max_video_bytes`, 5 GiB unless the operator set another).",
    VideoUploadCreateInvalid => "`request.invalid`: `errors` names `/title`, read from the metadata's `title` or else its `filename`: `name.empty` (neither is given, or it is empty after trimming), `name.too_long` (over 200 characters) or `text.contains_nul`.",
    TusVersionUnsupported => "`tus.version_unsupported`: the request has no `Tus-Resumable: 1.0.0`; `Tus-Version` lists the version the server speaks.",
    VideoUploadStorageUnavailable => "`storage.not_configured`: the server has no object storage configured, so it cannot store videos; the operator sets `STEPCLIMB_STORAGE_URL`. `org.none`: the instance has no organization yet.",
    ChunkBadRequest => "`request.malformed`: `Upload-Offset` is missing or not a number, `Content-Length` is not a number, or the body is longer or shorter than it says. `video.chunk_too_small`: the chunk is under 1 MiB and does not complete the upload; send larger chunks.",
    ChunkConflict => "`video.upload_offset_mismatch`: `Upload-Offset` is not the upload's offset; ask for it with `HEAD` and resume from there. `course.archived`: the course is archived and takes no content change.",
    ChunkLengthRequired => "`file.length_required`: the request has no `Content-Length`; send the chunk with its length.",
    ChunkTooLarge => "`file.too_large`: the chunk is over 64 MiB, or runs past the upload's `Upload-Length`.",
    ChunkTimeout => "`file.body_timeout`: the body sent nothing for 30 seconds; send the chunk again from the stored offset.",
    ChunkUnsupportedMediaType => "`request.unsupported_media_type`: the chunk is not sent with `Content-Type: application/offset+octet-stream`.",
    ChunkStorageUnavailable => "`storage.not_configured`: the server has no object storage configured. `file.uploads_busy`: the server is receiving as many uploads as it takes at once; try again shortly. `org.none`: the instance has no organization yet.",
    TerminateConflict => "`course.archived`: the course is archived and takes no content change. `video.status_invalid`: the upload is complete and its video is being processed; delete the video instead.",
}

/// `Tus-Resumable: 1.0.0` on every response of the tus routes, refusals included, and
/// `Tus-Version` on a 412 (tus 1.0.0, Headers). A layer on those routes only.
pub async fn tus_header(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(TUS_RESUMABLE, HeaderValue::from_static(TUS_VERSION));
    if response.status() == StatusCode::PRECONDITION_FAILED {
        response
            .headers_mut()
            .insert(TUS_VERSION_HEADER, HeaderValue::from_static(TUS_VERSION));
    }
    response
}

/// 412 unless the request says `Tus-Resumable: 1.0.0`.
fn require_tus(headers: &HeaderMap) -> Result<(), ApiError> {
    match headers.get(TUS_RESUMABLE) {
        Some(value) if value == TUS_VERSION => Ok(()),
        _ => Err(ApiError::new(
            StatusCode::PRECONDITION_FAILED,
            code::TUS_VERSION_UNSUPPORTED,
            "Send Tus-Resumable: 1.0.0; this server speaks tus 1.0.0 only.",
        )),
    }
}

/// A header that holds a number of bytes, digits only; `None` when it is absent.
fn byte_header(
    headers: &HeaderMap,
    name: &str,
    detail: &'static str,
) -> Result<Option<u64>, ApiError> {
    let Some(value) = headers.get(name) else {
        return Ok(None);
    };
    value
        .to_str()
        .ok()
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|v| v.parse::<u64>().ok())
        .map(Some)
        .ok_or_else(|| ApiError::malformed(detail))
}

/// The `Upload-Metadata` header's `filename` and `title` (tus 1.0.0, Creation): comma-separated
/// `key base64value` pairs, a key at most once, values UTF-8. Other keys are ignored.
fn metadata(headers: &HeaderMap) -> Result<(Option<String>, Option<String>), ApiError> {
    let bad =
        || ApiError::malformed("Upload-Metadata is not comma-separated key base64value pairs.");
    let Some(value) = headers.get(UPLOAD_METADATA) else {
        return Ok((None, None));
    };
    let value = value.to_str().map_err(|_| bad())?;
    let mut seen = Vec::new();
    let (mut filename, mut title) = (None, None);
    for pair in value.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (key, encoded) = pair.split_once(' ').unwrap_or((pair, ""));
        if key.is_empty() || seen.contains(&key) {
            return Err(bad());
        }
        seen.push(key);
        let decoded = STANDARD.decode(encoded.trim()).map_err(|_| bad())?;
        let text = String::from_utf8(decoded).map_err(|_| bad())?;
        match key {
            "filename" => filename = Some(text),
            "title" => title = Some(text),
            _ => {}
        }
    }
    Ok((filename, title))
}

/// What a creation answers: the upload's path and the video it fills.
#[derive(Clone, Debug, PartialEq, Serialize, utoipa::ToSchema)]
pub struct VideoUploadCreated {
    /// The upload's id; its path is the `Location` header.
    #[schema(value_type = String, format = "uuid")]
    pub upload_id: stepclimb_core::id::Uuid,
    pub video: Video,
}

/// Start a tus upload of a video into the course. The request has no body; `Upload-Metadata`
/// gives the video's title (`title`, or else `filename`). The video is created `uploading`,
/// attached to the course, and the whole length counts toward the plan's `video_storage` limit
/// from now on.
#[utoipa::path(
    post,
    path = "/api/v1/courses/{course_id}/video-uploads",
    operation_id = "createVideoUpload",
    tag = "videos",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(
        ("course_id" = String, Path, format = "uuid", description = "The course's id."),
        ("Tus-Resumable" = String, Header, description = "`1.0.0`."),
        ("Upload-Length" = u64, Header, description = "The video's length in bytes."),
        ("Upload-Metadata" = Option<String>, Header, description = "Comma-separated `key base64value` pairs; `title` and `filename` are read."),
    ),
    responses(
        (
            status = 201,
            description = "The upload is started; send the bytes with PATCH to its Location",
            body = VideoUploadCreated,
            headers(
                ("Location" = String, description = "The upload's path"),
                ("Tus-Resumable" = String, description = "`1.0.0`"),
            )
        ),
        (status = 400, response = VideoUploadCreateBadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = VideoUploadForbidden),
        (status = 404, response = VideoUploadCourseNotFound),
        (status = 409, response = VideoUploadCreateConflict),
        (status = 412, response = TusVersionUnsupported),
        (status = 413, response = VideoUploadCreateTooLarge),
        (status = 422, response = VideoUploadCreateInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = VideoUploadStorageUnavailable),
    )
)]
pub async fn create_video_upload(
    require: Require<CourseContentEdit, CourseTarget>,
    storage: Storage,
    entitlements: Entitlements,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    require_tus(&headers)?;
    let length = creation_length(&headers, storage.max_video_bytes())?;
    let title = creation_title(&headers)?;
    let course_id = require.target().id;
    let mut tx = begin(&state).await?;
    let course = recheck(&mut tx, &require, course_id, COURSE_NOT_FOUND).await?;
    if course.status == courses::Status::Archived {
        return Err(video_error(videos::Error::CourseArchived));
    }
    entitlements
        .check_room(&mut tx, Limit::VideoStorage, length)
        .await?;
    let new = NewUpload {
        course_id,
        title: &title,
        length,
        created_by: require.principal().user_id,
    };
    let scope = require.grants().scope();
    let (video, upload) = videos::create_upload(&mut tx, scope, &new, state.clock.now())
        .await
        .map_err(video_error)?;
    commit(tx).await?;
    created(upload.id, video)
}

/// The title `Upload-Metadata` gives (D2): `title`, or else `filename`; 422 without either or
/// with one `videos::title` refuses.
fn creation_title(headers: &HeaderMap) -> Result<String, ApiError> {
    let (filename, title) = metadata(headers)?;
    let title = title
        .or(filename)
        .ok_or_else(|| ApiError::invalid(vec![FieldError::at(&["title"], NAME_EMPTY)]))?;
    videos::title(&title).map_err(video_error)?;
    Ok(title)
}

/// The 201 of a creation: `Location` and [`VideoUploadCreated`] (D1).
fn created(
    upload_id: stepclimb_core::id::Uuid,
    video: videos::Video,
) -> Result<Response, ApiError> {
    let body = VideoUploadCreated {
        upload_id,
        video: Video::from_repo(video)?,
    };
    let location = format!("{VIDEO_UPLOADS_PATH}/{upload_id}");
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(body),
    )
        .into_response())
}

/// `Upload-Length`: 400 when absent, not a number or 0, 413 over `max`.
fn creation_length(headers: &HeaderMap, max: u64) -> Result<u64, ApiError> {
    let detail = "Send Upload-Length, the video's length in bytes, more than 0.";
    let length = byte_header(headers, UPLOAD_LENGTH, detail)?
        .filter(|n| *n > 0)
        .ok_or_else(|| ApiError::malformed(detail))?;
    if length > max {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            upload::code::TOO_LARGE,
            "The video is larger than this server accepts.",
        ));
    }
    Ok(length)
}

/// The offset and length headers of an upload.
fn offset_headers(upload: &Upload) -> [(&'static str, String); 2] {
    [
        (UPLOAD_OFFSET, upload.received.to_string()),
        (UPLOAD_LENGTH, upload.length.to_string()),
    ]
}

/// Ask how many bytes of the upload the server has: `Upload-Offset` is where the next chunk
/// starts. A response to `HEAD` has no body, so a refusal is told by its status only.
#[utoipa::path(
    head,
    path = "/api/v1/video-uploads/{upload_id}",
    operation_id = "getVideoUploadOffset",
    tag = "videos",
    params(
        ("upload_id" = String, Path, format = "uuid", description = "The upload's id."),
        ("Tus-Resumable" = String, Header, description = "`1.0.0`."),
    ),
    responses(
        (
            status = 200,
            description = "The upload's offset and length",
            headers(
                ("Upload-Offset" = u64, description = "The bytes stored, where the next chunk starts"),
                ("Upload-Length" = u64, description = "The upload's whole length"),
                ("Tus-Resumable" = String, description = "`1.0.0`"),
            )
        ),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = VideoUploadEditForbidden),
        (status = 404, response = VideoUploadNotFound),
        (status = 412, response = TusVersionUnsupported),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_video_upload_offset(
    require: Require<CourseContentEdit, VideoUploadTarget>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    require_tus(&headers)?;
    let upload = live_upload(&state, require.grants().scope(), require.target().id).await?;
    Ok((StatusCode::OK, offset_headers(&upload)).into_response())
}

/// The upload `id`, or 404 when it is gone or being deleted (D13).
async fn live_upload(
    state: &AppState,
    scope: OrgScope,
    id: stepclimb_core::id::Uuid,
) -> Result<Upload, ApiError> {
    videos::get_upload(&state.db, scope, id)
        .await
        .map_err(video_error)?
        .filter(|upload| upload.terminating.is_none())
        .ok_or_else(|| video_error(videos::Error::UploadNotFound))
}

/// Send the next chunk of the upload, starting at its `Upload-Offset`. A chunk is stored whole
/// or not at all: after a dropped connection, ask for the offset with `HEAD` and send again from
/// there. The chunk that completes the upload starts the video's processing.
#[utoipa::path(
    patch,
    path = "/api/v1/video-uploads/{upload_id}",
    operation_id = "appendVideoUpload",
    tag = "videos",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(
        ("upload_id" = String, Path, format = "uuid", description = "The upload's id."),
        ("Tus-Resumable" = String, Header, description = "`1.0.0`."),
        ("Upload-Offset" = u64, Header, description = "Where this chunk starts: the offset `HEAD` answers."),
    ),
    request_body(
        content = Vec<u8>,
        content_type = "application/offset+octet-stream",
        description = "The chunk's bytes, with `Content-Length`, at most 64 MiB."
    ),
    responses(
        (
            status = 204,
            description = "The chunk is stored",
            headers(
                ("Upload-Offset" = u64, description = "The bytes stored now"),
                ("Tus-Resumable" = String, description = "`1.0.0`"),
            )
        ),
        (status = 400, response = ChunkBadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = VideoUploadEditForbidden),
        (status = 404, response = VideoUploadNotFound),
        (status = 408, response = ChunkTimeout),
        (status = 409, response = ChunkConflict),
        (status = 411, response = ChunkLengthRequired),
        (status = 412, response = TusVersionUnsupported),
        (status = 413, response = ChunkTooLarge),
        (status = 415, response = ChunkUnsupportedMediaType),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = ChunkStorageUnavailable),
    )
)]
pub async fn append_video_upload(
    require: Require<CourseContentEdit, VideoUploadTarget>,
    storage: Storage,
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    require_tus(&headers)?;
    let chunk = Chunk::parse(&headers)?;
    let scope = require.grants().scope();
    let upload = live_upload(&state, scope, require.target().id).await?;
    chunk.fits(&upload)?;
    if chunk.length == 0 {
        return Ok((StatusCode::NO_CONTENT, offset_headers(&upload)).into_response());
    }
    let _in_flight = storage
        .try_start_upload()
        .ok_or_else(upload::uploads_busy)?;
    let temporary = stream_to_temporary(&storage, scope, body, chunk.length).await?;
    let appender = Appender {
        require: &require,
        storage: &storage,
        state: &state,
    };
    let stored = appender.store(&chunk, &temporary.path).await;
    appender.delete_object(&temporary.path).await;
    let (upload, failed) = stored?;
    if failed && let Err(error) = video_parts::finish(&state.db, &storage, scope, upload.id).await {
        let upload_id = upload.id;
        tracing::warn!(%error, %upload_id, "could not delete the parts of a video that is not a video; videos.cleanup will");
    }
    Ok((StatusCode::NO_CONTENT, offset_headers(&upload)).into_response())
}

/// A chunk's `Upload-Offset` and length.
struct Chunk {
    offset: u64,
    length: u64,
}

impl Chunk {
    /// 415 without `application/offset+octet-stream`, 400 without a numeric `Upload-Offset`,
    /// and `Content-Length`'s 411, 400 and 413 over [`MAX_CHUNK_BYTES`].
    fn parse(headers: &HeaderMap) -> Result<Self, ApiError> {
        let essence = headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| {
                v.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase()
            });
        if essence.as_deref() != Some(OFFSET_OCTET_STREAM) {
            return Err(ApiError::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                REQUEST_UNSUPPORTED_MEDIA_TYPE,
                "Send the chunk with Content-Type: application/offset+octet-stream.",
            ));
        }
        let detail = "Send Upload-Offset, the offset the chunk starts at.";
        let offset = byte_header(headers, UPLOAD_OFFSET, detail)?
            .ok_or_else(|| ApiError::malformed(detail))?;
        let length = content_length(headers, MAX_CHUNK_BYTES)?;
        Ok(Self { offset, length })
    }

    /// 409 when the chunk does not start at the stored offset, 413 when it runs past the
    /// upload's length, 400 when it is under [`MIN_CHUNK_BYTES`] and does not complete the
    /// upload. An empty chunk stores nothing and passes (D6).
    fn fits(&self, upload: &Upload) -> Result<(), ApiError> {
        if self.offset != upload.received {
            return Err(video_error(videos::Error::OffsetMismatch));
        }
        let remaining = upload.length - upload.received;
        if self.length > remaining {
            return Err(ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                upload::code::TOO_LARGE,
                "The chunk runs past the upload's Upload-Length.",
            ));
        }
        if self.length > 0 && self.length < remaining && self.length < MIN_CHUNK_BYTES {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                code::CHUNK_TOO_SMALL,
                "A chunk that does not complete the upload must be at least 1 MiB.",
            ));
        }
        Ok(())
    }
}

/// One chunk request's extractors.
struct Appender<'a> {
    require: &'a Require<CourseContentEdit, VideoUploadTarget>,
    storage: &'a Storage,
    state: &'a AppState,
}

impl Appender<'_> {
    fn scope(&self) -> OrgScope {
        self.require.grants().scope()
    }

    /// B6 and B7's transaction: the chunk at `temporary` copied to its part and counted, and a
    /// complete upload's video moved on. Whether the video failed as not a video.
    async fn store(&self, chunk: &Chunk, temporary: &Path) -> Result<(Upload, bool), ApiError> {
        let (scope, target) = (self.scope(), self.require.target());
        let mut tx = begin(self.state).await?;
        let course = recheck_shared(
            &mut tx,
            self.require,
            target.course.id,
            VIDEO_UPLOAD_NOT_FOUND,
        )
        .await?;
        if course.status == courses::Status::Archived {
            return Err(video_error(videos::Error::CourseArchived));
        }
        let locked = videos::lock_upload(&mut tx, scope, target.id)
            .await
            .map_err(video_error)?
            .filter(|upload| upload.terminating.is_none())
            .ok_or_else(|| video_error(videos::Error::UploadNotFound))?;
        chunk.fits(&locked)?;
        let part = self
            .storage
            .video_part_path(scope, locked.video_id, chunk.offset);
        self.storage
            .store()
            .copy(temporary, &part)
            .await
            .map_err(|error| ApiError::internal(&error))?;
        let now = self.state.clock.now();
        let upload = videos::advance(&mut tx, scope, target.id, (chunk.offset, chunk.length), now)
            .await
            .map_err(video_error)?;
        let failed = if upload.is_complete() {
            self.complete(&mut tx, &upload).await?
        } else {
            false
        };
        commit(tx).await?;
        Ok((upload, failed))
    }

    /// B7: sniff the first bytes of the complete `upload`; a video becomes `processing` with its
    /// transcode job enqueued in `tx`, anything else `failed` with `video.not_a_video` and its
    /// upload marked terminating (D7). Whether it failed.
    async fn complete(
        &self,
        tx: &mut Transaction<'static, Postgres>,
        upload: &Upload,
    ) -> Result<bool, ApiError> {
        let scope = self.scope();
        let head = read_head(self.storage, scope, upload)
            .await
            .map_err(|error| ApiError::internal(&error))?;
        let is_video = sniff::video(&head).is_some();
        let now = self.state.clock.now();
        let error_code = (!is_video).then_some(videos::NOT_A_VIDEO);
        videos::finish_upload(tx, scope, upload.video_id, error_code, now)
            .await
            .map_err(video_error)?;
        if !is_video {
            videos::mark_terminating(tx, scope, upload.id, Terminating::Parts, now)
                .await
                .map_err(video_error)?;
        } else {
            let payload = serde_json::json!({ "video_id": upload.video_id });
            let key = upload.video_id.to_string();
            let job = NewJob::org(&TRANSCODE, scope, &payload)
                .and_then(|job| job.dedupe(&key))
                .map_err(|error| ApiError::internal(&error))?;
            enqueue(tx, job, now)
                .await
                .map_err(|error| ApiError::internal(&error))?;
        }
        Ok(!is_video)
    }

    async fn delete_object(&self, location: &Path) {
        if let Err(error) = self.storage.store().delete(location).await {
            tracing::warn!(%error, key = %location, "could not delete a video chunk's temporary object");
        }
    }
}

/// The upload's first [`sniff::HEAD_BYTES`] (or all of it, if shorter), read part after part:
/// each part's key is its offset, so the next part starts where the bytes read so far end.
async fn read_head(
    storage: &Storage,
    scope: OrgScope,
    upload: &Upload,
) -> Result<Vec<u8>, object_store::Error> {
    // 8 KiB fits a u64 on every target.
    let window = u64::try_from(sniff::HEAD_BYTES).expect("HEAD_BYTES fits a u64");
    let want = window.min(upload.received);
    let mut head = Vec::new();
    let mut offset = 0;
    while offset < want {
        let part = storage.video_part_path(scope, upload.video_id, offset);
        // A range past the object's end reads to its end (object_store's `GetRange::Bounded`).
        let bytes = storage.store().get_range(&part, 0..want - offset).await?;
        if bytes.is_empty() {
            break;
        }
        // A read of at most `want` bytes fits a u64.
        offset += u64::try_from(bytes.len()).expect("a read of at most 8 KiB fits a u64");
        head.extend_from_slice(&bytes);
    }
    Ok(head)
}

/// End the upload: its parts, its video and the video's attachment are deleted. From the answer
/// on the upload is gone; its parts are deleted right after, or by the hourly cleanup if that
/// fails. An upload whose video is already processing cannot be ended; delete the video instead.
#[utoipa::path(
    delete,
    path = "/api/v1/video-uploads/{upload_id}",
    operation_id = "deleteVideoUpload",
    tag = "videos",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(
        ("upload_id" = String, Path, format = "uuid", description = "The upload's id."),
        ("Tus-Resumable" = String, Header, description = "`1.0.0`."),
    ),
    responses(
        (
            status = 204,
            description = "The upload and its video are deleted",
            headers(("Tus-Resumable" = String, description = "`1.0.0`"))
        ),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = VideoUploadEditForbidden),
        (status = 404, response = VideoUploadNotFound),
        (status = 409, response = TerminateConflict),
        (status = 412, response = TusVersionUnsupported),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = VideoUploadStorageUnavailable),
    )
)]
pub async fn delete_video_upload(
    require: Require<CourseContentEdit, VideoUploadTarget>,
    storage: Storage,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    require_tus(&headers)?;
    let (scope, target) = (require.grants().scope(), require.target());
    let mut tx = begin(&state).await?;
    let course = recheck(&mut tx, &require, target.course.id, VIDEO_UPLOAD_NOT_FOUND).await?;
    if course.status == courses::Status::Archived {
        return Err(video_error(videos::Error::CourseArchived));
    }
    let upload = videos::lock_upload(&mut tx, scope, target.id)
        .await
        .map_err(video_error)?
        .filter(|upload| upload.terminating.is_none())
        .ok_or_else(|| video_error(videos::Error::UploadNotFound))?;
    let video = videos::get(&mut tx, scope, upload.video_id)
        .await
        .map_err(video_error)?
        .ok_or_else(|| video_error(videos::Error::UploadNotFound))?;
    if !matches!(
        video.status,
        videos::Status::Uploading | videos::Status::Failed
    ) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            video_code::STATUS_INVALID,
            "The upload is complete and its video is being processed; delete the video instead.",
        ));
    }
    videos::mark_terminating(
        &mut tx,
        scope,
        upload.id,
        Terminating::Video,
        state.clock.now(),
    )
    .await
    .map_err(video_error)?;
    commit(tx).await?;
    if let Err(error) = video_parts::finish(&state.db, &storage, scope, upload.id).await {
        let upload_id = upload.id;
        tracing::warn!(%error, %upload_id, "could not finish deleting an ended upload; videos.cleanup will");
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, HeaderValue::from_str(value).unwrap());
        }
        map
    }

    fn refused(error: ApiError) -> (u16, String) {
        let problem = error.problem();
        (problem.status, problem.code)
    }

    #[test]
    fn only_tus_1_0_0_is_spoken() {
        assert!(require_tus(&headers(&[("tus-resumable", "1.0.0")])).is_ok());
        for value in [None, Some("0.2.2"), Some("1.0"), Some("")] {
            let sent: Vec<_> = value.map(|v| ("tus-resumable", v)).into_iter().collect();
            assert_eq!(
                refused(require_tus(&headers(&sent)).unwrap_err()),
                (412, "tus.version_unsupported".to_owned()),
                "{value:?}"
            );
        }
    }

    #[test]
    fn metadata_reads_title_and_filename_and_ignores_other_keys() {
        // "Week 1" and "w1.mp4" in base64.
        let sent = headers(&[(
            "upload-metadata",
            "title V2VlayAx,filename dzEubXA0, filetype dmlkZW8vbXA0,is_confidential",
        )]);
        assert_eq!(
            metadata(&sent).unwrap(),
            (Some("w1.mp4".to_owned()), Some("Week 1".to_owned()))
        );
        assert_eq!(metadata(&HeaderMap::new()).unwrap(), (None, None));
        for bad in [
            "title V2VlayAx,title V2VlayAx",
            "title !!!",
            "title //79",
            "title V2VlayAx extra",
        ] {
            assert_eq!(
                refused(metadata(&headers(&[("upload-metadata", bad)])).unwrap_err()),
                (400, "request.malformed".to_owned()),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn the_creation_length_is_required_positive_and_at_most_the_maximum() {
        let length = |value: &str| creation_length(&headers(&[("upload-length", value)]), 100);
        assert_eq!(length("100").unwrap(), 100);
        assert_eq!(
            refused(length("101").unwrap_err()),
            (413, "file.too_large".to_owned())
        );
        for bad in ["0", "-1", "1e3", ""] {
            assert_eq!(
                refused(length(bad).unwrap_err()),
                (400, "request.malformed".to_owned()),
                "{bad:?}"
            );
        }
        assert_eq!(
            refused(creation_length(&HeaderMap::new(), 100).unwrap_err()),
            (400, "request.malformed".to_owned())
        );
    }

    #[test]
    fn a_chunk_needs_its_type_offset_and_length() {
        let ok = [
            ("content-type", "application/offset+octet-stream"),
            ("upload-offset", "5"),
            ("content-length", "3"),
        ];
        let chunk = Chunk::parse(&headers(&ok)).unwrap();
        assert_eq!((chunk.offset, chunk.length), (5, 3));
        let mut json = ok;
        json[0] = ("content-type", "application/json");
        assert_eq!(
            refused(Chunk::parse(&headers(&json)).err().unwrap()),
            (415, "request.unsupported_media_type".to_owned())
        );
        let no_offset = [ok[0], ok[2]];
        assert_eq!(
            refused(Chunk::parse(&headers(&no_offset)).err().unwrap()),
            (400, "request.malformed".to_owned())
        );
        let mut too_long = ok;
        too_long[2] = ("content-length", "67108865");
        assert_eq!(
            refused(Chunk::parse(&headers(&too_long)).err().unwrap()),
            (413, "file.too_large".to_owned())
        );
    }

    #[test]
    fn a_chunk_fits_only_at_the_offset_and_within_the_length() {
        let upload = Upload {
            id: stepclimb_core::id::Uuid::nil(),
            video_id: stepclimb_core::id::Uuid::nil(),
            course_id: stepclimb_core::id::Uuid::nil(),
            length: 10,
            received: 4,
            terminating: None,
            updated_at: std::time::SystemTime::UNIX_EPOCH,
        };
        let chunk = |offset, length| Chunk { offset, length };
        assert!(chunk(4, 6).fits(&upload).is_ok());
        assert!(chunk(4, 0).fits(&upload).is_ok());
        assert_eq!(
            refused(chunk(3, 1).fits(&upload).unwrap_err()),
            (409, "video.upload_offset_mismatch".to_owned())
        );
        assert_eq!(
            refused(chunk(4, 7).fits(&upload).unwrap_err()),
            (413, "file.too_large".to_owned())
        );
    }

    #[test]
    fn a_chunk_under_1_mib_passes_only_when_it_completes_the_upload() {
        const MIB: u64 = 1024 * 1024;
        let upload = Upload {
            id: stepclimb_core::id::Uuid::nil(),
            video_id: stepclimb_core::id::Uuid::nil(),
            course_id: stepclimb_core::id::Uuid::nil(),
            length: 3 * MIB,
            received: MIB,
            terminating: None,
            updated_at: std::time::SystemTime::UNIX_EPOCH,
        };
        let chunk = |length| Chunk {
            offset: MIB,
            length,
        };
        assert!(chunk(MIB).fits(&upload).is_ok());
        assert!(chunk(2 * MIB).fits(&upload).is_ok(), "the last chunk");
        assert_eq!(
            refused(chunk(MIB - 1).fits(&upload).unwrap_err()),
            (400, "video.chunk_too_small".to_owned())
        );
        let short = Upload {
            length: MIB + 10,
            ..upload
        };
        assert!(chunk(10).fits(&short).is_ok(), "a short last chunk");
        assert!(
            chunk(0).fits(&short).is_ok(),
            "an empty chunk stores nothing"
        );
        assert_eq!(
            refused(chunk(9).fits(&short).unwrap_err()),
            (400, "video.chunk_too_small".to_owned())
        );
    }
}
