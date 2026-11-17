// SPDX-License-Identifier: AGPL-3.0-or-later
//! Uploading a course file (docs/specs/m1/06-files.md B3 to B7).
//!
//! The route takes the file as the raw request body. It runs in three steps:
//!
//! 1. Before reading a byte ([`precheck`]): `Content-Length` is required (411) and at most the
//!    instance's `max_file_bytes` (413); the folder and name keep B9's rules (422); the course is
//!    not archived and the name is free in the folder (409); and the length fits the plan's
//!    `storage` limit (403), unless the organization holds a file of exactly that length, which
//!    the upload may turn out to repeat at no cost.
//! 2. [`stream`] takes one of the server's places for uploads in flight
//!    (`storage::UploadLimits`, 16 unless set; 503 `file.uploads_busy` when none is free) and
//!    writes the body to a temporary object `orgs/<org_id>/uploads/<uuid>` with a multipart
//!    upload, feeding every chunk to `files::Sha256Hasher` as it goes and keeping the first 8 KiB
//!    for `stepclimb_core::sniff`. A body longer or shorter than its `Content-Length` answers
//!    400, one that sends no frame for 30 seconds answers 408 `file.body_timeout`, and either
//!    way the multipart upload is aborted.
//! 3. [`store`] records the file (B7). In one transaction it locks the hash
//!    (`files::lock_hash`), reads the organization's `files` row for it `FOR KEY SHARE` and, for
//!    a new hash, copies the temporary object to the file's key, holding no course or plan row
//!    while the store copies. Only then does it check the permission again under the course lock,
//!    run `check_room` with the real size or 0 when the row exists, insert the `files` row of a
//!    new hash and the `course_files` row, and commit. A failure before the commit, a full
//!    `storage` limit included, deletes the copy of a new hash while the hash lock is still held,
//!    so no other upload can have committed a row naming it. The temporary object is deleted
//!    last, whatever happened.
//!
//! A server that stops in the middle of an S3 upload leaves an incomplete multipart upload in the
//! bucket. It is not an object, so `files.cleanup` never lists it: the operator removes those
//! with the bucket's lifecycle rule (`deploy/compose/README.md`).
//!
//! The hash lock is held from before the lookup until the commit, so two uploads of the same
//! new bytes take turns: the second finds the first one's row and adds nothing, and the unique
//! index on `(org_id, sha256)` is never raced. `files.cleanup` takes the same lock before it
//! deletes a row and its object ([`crate::files_cleanup`]).

use std::time::Duration;

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use http_body_util::BodyExt as _;
use object_store::{ObjectStoreExt as _, WriteMultipart, path::Path};
use serde::Deserialize;
use sqlx::{Postgres, Transaction};
use stepclimb_core::{
    OrgScope, course_files, courses,
    feature::Limit,
    files::{self, Hashed, NewFile, Sha256Hasher},
    id::new_id,
    perm::CourseContentEdit,
    sniff,
};
use utoipa::IntoParams;

use crate::{
    AppState,
    course_files::{COURSE_FILES_PATH, CourseFile, CourseFileInvalid, file_error, folder, name},
    entitlements::{self, Entitlements},
    error::ApiError,
    extract::ApiQuery,
    modules::{begin, commit, recheck},
    openapi::problem_responses,
    require::{CourseTarget, Require, code::COURSE_NOT_FOUND},
    storage::Storage,
};

/// The codes this module produces besides [`crate::course_files::code`]'s.
pub mod code {
    /// The request has no `Content-Length` (411).
    pub const LENGTH_REQUIRED: &str = "file.length_required";
    /// `Content-Length` is over the instance's `max_file_bytes` (413).
    pub const TOO_LARGE: &str = "file.too_large";
    /// The body sent no frame for `UploadLimits::idle` (408).
    pub const BODY_TIMEOUT: &str = "file.body_timeout";
    /// The server is streaming as many uploads as it takes at once (503).
    pub const UPLOADS_BUSY: &str = "file.uploads_busy";
}

/// How many parts of one upload may be on their way to the store when the next chunk is
/// written. Writing a chunk can start one more part, so an upload holds at most three 5 MiB parts
/// (two in flight, one filling) and one body frame: about 15 MiB.
const PARTS_IN_FLIGHT: usize = 2;

problem_responses! {
    UploadCourseNotFound => "`course.not_found`: no course with this id exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined.",
    UploadForbidden => "`permission.denied`: you can see the course but do not hold `course.content_edit` at a scope containing it. `entitlement.limit_reached`: the file would take the organization past its plan's `storage` limit; `limit` and `max` say which and how much. `auth.csrf_refused`: the request did not come from this site.",
    UploadConflict => "`course.archived`: the course is archived and takes no content change. `file.name_taken`: the folder already holds a file with this name.",
    UploadBadRequest => "`request.malformed`: `Content-Length` is not a number, the body is longer or shorter than it says, or a query parameter does not parse.",
    UploadLengthRequired => "`file.length_required`: the request has no `Content-Length`; send the file with its length.",
    UploadTooLarge => "`file.too_large`: `Content-Length` is over the largest file this server accepts (`max_file_bytes`, 100 MiB unless the operator set another).",
    UploadTimeout => "`file.body_timeout`: the body sent nothing for 30 seconds; send the file again.",
    UploadStorageUnavailable => "`storage.not_configured`: the server has no object storage configured, so it cannot store files; the operator sets `STEPCLIMB_STORAGE_URL`. `file.uploads_busy`: the server is receiving as many uploads as it takes at once; try again shortly. `org.none`: the instance has no organization yet.",
}

/// Where to put an uploaded file.
#[derive(Clone, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct UploadParams {
    /// The folder, such as `/handouts`; `/`, the course's top folder, when omitted.
    pub folder: Option<String>,
    /// The file's name in the folder.
    pub name: String,
}

/// Upload a file into a folder of the course. The body is the file itself, with its
/// `Content-Length`; its `Content-Type` is ignored: the server reads the type from the bytes.
/// The same bytes the organization already stores are kept once and add nothing to its storage.
#[utoipa::path(
    post,
    path = "/api/v1/courses/{course_id}/files",
    operation_id = "uploadCourseFile",
    tag = "files",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(
        ("course_id" = String, Path, format = "uuid", description = "The course's id."),
        UploadParams,
    ),
    request_body(
        content = Vec<u8>,
        content_type = "application/octet-stream",
        description = "The file's bytes, with `Content-Length`."
    ),
    responses(
        (
            status = 201,
            description = "The file is stored and placed in the folder",
            body = CourseFile,
            headers(("Location" = String, description = "The course file's path"))
        ),
        (status = 400, response = UploadBadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = UploadForbidden),
        (status = 404, response = UploadCourseNotFound),
        (status = 408, response = UploadTimeout),
        (status = 409, response = UploadConflict),
        (status = 411, response = UploadLengthRequired),
        (status = 413, response = UploadTooLarge),
        (status = 422, response = CourseFileInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = UploadStorageUnavailable),
    )
)]
pub async fn upload_course_file(
    require: Require<CourseContentEdit, CourseTarget>,
    storage: Storage,
    entitlements: Entitlements,
    State(state): State<AppState>,
    ApiQuery(params): ApiQuery<UploadParams>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let upload = Upload {
        require: &require,
        storage: &storage,
        entitlements: &entitlements,
        state: &state,
    };
    let length = content_length(&headers, storage.max_file_bytes())?;
    let place = Place::parse(&params)?;
    upload.precheck(&place, length).await?;
    let _in_flight = storage.try_start_upload().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            code::UPLOADS_BUSY,
            "The server is receiving as many uploads as it takes at once; try again shortly.",
        )
    })?;
    let streamed = upload.stream(body, length).await?;
    let stored = upload.store(&place, &streamed).await;
    upload.delete_temporary(&streamed.temporary).await;
    let file = CourseFile::from_repo(stored?)?;
    let location = format!("{COURSE_FILES_PATH}/{}", file.id);
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(file),
    )
        .into_response())
}

/// The request's `Content-Length`: 411 without one, 400 when it is not a number, 413 over `max`.
fn content_length(headers: &HeaderMap, max: u64) -> Result<u64, ApiError> {
    let value = headers.get(header::CONTENT_LENGTH).ok_or_else(|| {
        ApiError::new(
            StatusCode::LENGTH_REQUIRED,
            code::LENGTH_REQUIRED,
            "Send the file with its Content-Length.",
        )
    })?;
    let length = value
        .to_str()
        .ok()
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|v| v.parse::<u64>().ok())
        .ok_or_else(|| ApiError::malformed("Content-Length is not a number of bytes."))?;
    if length > max {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            code::TOO_LARGE,
            "The file is larger than this server accepts.",
        ));
    }
    Ok(length)
}

/// The folder and name the file goes to.
struct Place {
    folder: course_files::Folder,
    name: course_files::Name,
}

impl Place {
    fn parse(params: &UploadParams) -> Result<Self, ApiError> {
        let folder = match &params.folder {
            Some(value) => folder("folder", value)?,
            None => course_files::Folder::root(),
        };
        Ok(Self {
            folder,
            name: name(&params.name)?,
        })
    }

    fn pair(&self) -> (&course_files::Folder, &course_files::Name) {
        (&self.folder, &self.name)
    }
}

/// What [`Upload::stream`] wrote: where, and what the server computed over the bytes.
struct Streamed {
    temporary: Path,
    hashed: Hashed,
    mime: &'static str,
}

/// One upload request's extractors.
struct Upload<'a> {
    require: &'a Require<CourseContentEdit, CourseTarget>,
    storage: &'a Storage,
    entitlements: &'a Entitlements,
    state: &'a AppState,
}

impl Upload<'_> {
    fn scope(&self) -> OrgScope {
        self.require.grants().scope()
    }

    fn course_id(&self) -> stepclimb_core::id::Uuid {
        self.require.target().id
    }

    /// B4's checks that need the database, before any byte is read: 409 for an archived course
    /// or a name already in the folder, 403 for a length the `storage` limit has no room for
    /// unless the organization holds a file of that length (which the upload may repeat, adding
    /// nothing). Nothing stays locked: [`Self::store`] checks again.
    async fn precheck(&self, place: &Place, length: u64) -> Result<(), ApiError> {
        let (scope, db) = (self.scope(), &self.state.db);
        let course = courses::get(db, scope, self.course_id())
            .await
            .map_err(|error| ApiError::internal(&error))?
            .ok_or_else(|| file_error(course_files::Error::CourseNotFound))?;
        if course.status == courses::Status::Archived {
            return Err(file_error(course_files::Error::CourseArchived));
        }
        let taken = course_files::name_taken(db, scope, course.id, place.pair())
            .await
            .map_err(file_error)?;
        if taken {
            return Err(file_error(course_files::Error::NameTaken));
        }
        let mut tx = begin(self.state).await?;
        let room = self
            .entitlements
            .check_room(&mut tx, Limit::Storage, length)
            .await;
        if let Err(full) = room {
            let maybe_repeat = full.code() == entitlements::code::LIMIT_REACHED
                && files::any_of_size(&mut *tx, scope, length)
                    .await
                    .map_err(|error| ApiError::internal(&error))?;
            if !maybe_repeat {
                return Err(full);
            }
        }
        tx.rollback()
            .await
            .map_err(|error| ApiError::internal(&error))
    }

    /// Write the body to a new temporary object, hashing and counting it (B5) and sniffing its
    /// first bytes (B6). A body of another length than `length` answers 400; on any failure the
    /// multipart upload is aborted, so no temporary object is left.
    async fn stream(&self, body: Body, length: u64) -> Result<Streamed, ApiError> {
        let temporary = self.storage.upload_path(self.scope(), new_id());
        let multipart = self
            .storage
            .store()
            .put_multipart(&temporary)
            .await
            .map_err(|error| ApiError::internal(&error))?;
        let mut writer = WriteMultipart::new(multipart);
        let idle = self.storage.upload_limits().idle;
        let copied = copy_body(body, length, idle, &mut writer).await;
        let (hasher, head) = match copied {
            Ok(done) => done,
            Err(error) => {
                if let Err(abort) = writer.abort().await {
                    tracing::warn!(error = %abort, key = %temporary, "could not abort an upload");
                }
                return Err(error);
            }
        };
        writer
            .finish()
            .await
            .map_err(|error| ApiError::internal(&error))?;
        let complete = u64::try_from(head.len()).is_ok_and(|n| n == length);
        Ok(Streamed {
            temporary,
            hashed: hasher.finish(),
            mime: sniff::sniff(&head, complete),
        })
    }

    /// B7's transaction. A failure before the commit removes the copy of a new hash.
    async fn store(
        &self,
        place: &Place,
        streamed: &Streamed,
    ) -> Result<course_files::CourseFile, ApiError> {
        let (scope, sha256) = (self.scope(), streamed.hashed.sha256());
        let mut tx = begin(self.state).await?;
        files::lock_hash(&mut tx, scope, sha256)
            .await
            .map_err(|error| ApiError::internal(&error))?;
        let existing = files::find_by_hash(&mut tx, scope, sha256)
            .await
            .map_err(|error| ApiError::internal(&error))?
            .map(|file| file.id);
        let copy = self.storage.file_path(scope, sha256);
        let recorded = match existing {
            Some(_) => self.record(&mut tx, place, streamed, existing).await,
            None => match self.storage.store().copy(&streamed.temporary, &copy).await {
                Ok(()) => self.record(&mut tx, place, streamed, None).await,
                Err(error) => Err(ApiError::internal(&error)),
            },
        };
        match recorded {
            Ok(file) => {
                commit(tx).await?;
                Ok(file)
            }
            Err(error) => {
                if existing.is_none() {
                    // The hash lock is still held: no other upload can have a row naming it.
                    self.delete_object(&copy).await;
                }
                if let Err(rollback) = tx.rollback().await {
                    tracing::warn!(error = %rollback, "could not roll back a failed upload");
                }
                Err(error)
            }
        }
    }

    /// The rows of B7 once the bytes are at their key: the permission again under the course
    /// lock, the `storage` limit with the bytes this upload adds (none for `existing`), the
    /// `files` row of a new hash and the `course_files` row.
    async fn record(
        &self,
        tx: &mut Transaction<'static, Postgres>,
        place: &Place,
        streamed: &Streamed,
        existing: Option<stepclimb_core::id::Uuid>,
    ) -> Result<course_files::CourseFile, ApiError> {
        recheck(tx, self.require, self.course_id(), COURSE_NOT_FOUND).await?;
        let adding = match existing {
            Some(_) => 0,
            None => streamed.hashed.size(),
        };
        self.entitlements
            .check_room(tx, Limit::Storage, adding)
            .await?;
        let file_id = match existing {
            Some(id) => id,
            None => {
                let new = NewFile {
                    hashed: streamed.hashed,
                    mime: streamed.mime.to_owned(),
                };
                files::insert(&mut **tx, self.scope(), &new, self.state.clock.now())
                    .await
                    .map_err(|error| ApiError::internal(&error))?
                    .id
            }
        };
        self.place(tx, place, file_id).await
    }

    /// The `course_files` row of the stored file `file_id` in the course.
    async fn place(
        &self,
        tx: &mut sqlx::PgConnection,
        place: &Place,
        file_id: stepclimb_core::id::Uuid,
    ) -> Result<course_files::CourseFile, ApiError> {
        let now = self.state.clock.now();
        course_files::create(
            tx,
            self.scope(),
            self.course_id(),
            file_id,
            place.pair(),
            now,
        )
        .await
        .map_err(file_error)
    }

    /// B7 step 3. A failed delete is logged; `files.cleanup` removes temporary objects a day old.
    async fn delete_temporary(&self, temporary: &Path) {
        self.delete_object(temporary).await;
    }

    async fn delete_object(&self, location: &Path) {
        if let Err(error) = self.storage.store().delete(location).await {
            tracing::warn!(%error, key = %location, "could not delete an object of an upload");
        }
    }
}

/// Every chunk of `body` fed to the hasher and the writer in order, with back pressure on the
/// store; the hasher and the first [`sniff::HEAD_BYTES`]. 400 when the body is longer or
/// shorter than `length` or breaks off, 408 when it sends no frame for `idle`.
async fn copy_body(
    mut body: Body,
    length: u64,
    idle: Duration,
    writer: &mut WriteMultipart,
) -> Result<(Sha256Hasher, Vec<u8>), ApiError> {
    let wrong_length =
        || ApiError::malformed("The body is not as long as its Content-Length says.");
    let stalled = || {
        ApiError::new(
            StatusCode::REQUEST_TIMEOUT,
            code::BODY_TIMEOUT,
            "The body sent nothing for too long; send the file again.",
        )
    };
    let mut hasher = Sha256Hasher::new();
    let mut head = Vec::new();
    while let Some(frame) = tokio::time::timeout(idle, body.frame())
        .await
        .map_err(|_| stalled())?
    {
        let frame = frame.map_err(|_| ApiError::malformed("The body broke off."))?;
        let Ok(chunk) = frame.into_data() else {
            continue;
        };
        // hyper stops an HTTP/1.1 body at its Content-Length; this keeps the bound whatever
        // transport delivers the body, before a byte past it reaches the store.
        if !fits(hasher.bytes(), chunk.len(), length) {
            return Err(wrong_length());
        }
        let wanted = sniff::HEAD_BYTES
            .saturating_sub(head.len())
            .min(chunk.len());
        head.extend_from_slice(&chunk[..wanted]);
        hasher.update(&chunk);
        writer
            .wait_for_capacity(PARTS_IN_FLIGHT)
            .await
            .map_err(|error| ApiError::internal(&error))?;
        writer.put(chunk);
    }
    if hasher.bytes() != length {
        return Err(wrong_length());
    }
    Ok((hasher, head))
}

/// Whether a chunk of `chunk` bytes after `seen` keeps the body within `length`.
fn fits(seen: u64, chunk: usize, length: u64) -> bool {
    u64::try_from(chunk)
        .ok()
        .and_then(|n| seen.checked_add(n))
        .is_some_and(|after| after <= length)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn with_length(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(value).unwrap(),
        );
        headers
    }

    #[test]
    fn a_chunk_fits_only_within_the_length() {
        assert!(fits(0, 3, 3));
        assert!(fits(3, 0, 3));
        assert!(!fits(3, 1, 3));
        assert!(!fits(0, 4, 3));
        assert!(!fits(u64::MAX, 1, u64::MAX));
    }

    #[test]
    fn the_length_is_required_numeric_and_at_most_the_maximum() {
        let refused = |headers: &HeaderMap| {
            let problem = content_length(headers, 100).unwrap_err().problem();
            (problem.status, problem.code)
        };
        assert_eq!(
            refused(&HeaderMap::new()),
            (411, "file.length_required".to_owned())
        );
        assert_eq!(
            refused(&with_length("101")),
            (413, "file.too_large".to_owned())
        );
        for bad in ["", "-1", "+5", "1e3", "ten", "99999999999999999999"] {
            assert_eq!(
                refused(&with_length(bad)),
                (400, "request.malformed".to_owned()),
                "{bad:?}"
            );
        }
        assert_eq!(content_length(&with_length("100"), 100).unwrap(), 100);
        assert_eq!(content_length(&with_length("0"), 100).unwrap(), 0);
    }
}
