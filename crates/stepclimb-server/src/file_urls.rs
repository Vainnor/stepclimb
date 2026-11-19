// SPDX-License-Identifier: AGPL-3.0-or-later
//! Signed file URLs (docs/specs/m1/06-files.md B14 to B16, Q6, Q7): the token that lets a
//! browser fetch one course file for a short while, and the two routes that issue it.
//!
//! A token seals `{ org_id, file_id, course_file_id, disposition, expires_at }` with the
//! encryption key ring under the context `file_url` ([`seal`]), base64url-encoded. Its holder can
//! neither read nor change it: a changed byte, another ring or another context makes [`open`]
//! fail. It is valid for [`TTL`] after issue by the request's clock. Whoever holds the URL can
//! fetch the file until then, which is what lets `<img>`, `<iframe>` and the browser's PDF viewer
//! load it without a cookie; [`crate::file_delivery`] serves it.
//!
//! `POST /api/v1/items/{item_id}/file-url` takes `Require<perm::CourseView, ItemTarget>` and, for
//! a caller without `course.view_all`, the item's visibility and lock check (spec 03 B14, spec 04
//! B13), with the caller's own completed items when they are a learner and none otherwise.
//! `POST /api/v1/course-files/{course_file_id}/url` takes `Require<perm::CourseViewAll,
//! CourseFileTarget>`. Both take [`Storage`] after their `Require`, so only an allowed caller
//! learns that storage is not configured (B1a).

use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{Json, extract::State};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope, course_files,
    id::Uuid,
    items,
    perm::{CourseView, CourseViewAll},
    progress, unlock,
};
use utoipa::ToSchema;

use crate::{
    AppState,
    error::{ApiError, FieldError},
    extract::ApiJson,
    modules::content_error,
    openapi::problem_responses,
    outline,
    progress::progress_error,
    require::{
        CourseFileTarget, ItemTarget, Require,
        code::{FILE_NOT_FOUND, ITEM_NOT_FOUND},
        not_found_as,
    },
    storage::Storage,
    timestamp,
};

/// How long a signed URL is valid after it is issued (Q7).
pub const TTL: Duration = Duration::from_secs(10 * 60);

/// The path every signed URL starts with; `{FILES_PATH}/{token}`.
pub const FILES_PATH: &str = "/files";

/// The encryption context of a token: a ring's ciphertext for any other purpose never opens as
/// one.
const CONTEXT: &[u8] = b"file_url";

/// The longest token [`open`] tries to decode. A sealed payload is under 300 bytes; anything
/// much longer is not a token and is refused before any work.
const MAX_TOKEN_CHARS: usize = 1024;

/// The codes this module produces besides the targets' 404s.
pub mod code {
    /// The item is not a `file` item (a 422 field code at the body's root).
    pub const NOT_A_FILE: &str = "item.not_a_file";
}

problem_responses! {
    FileUrlNotFound => "`item.not_found`: no item with this id exists in the organization, or you cannot see it: its course is out of your reach, a draft course or a template, or the item or its module is unpublished, and you do not hold `course.view_all` at the course. `file.not_found`: the item's course file no longer exists. `org.not_found`: the request's organization cannot be determined.",
    FileUrlForbidden => "`permission.denied`: you do not hold `course.view` at a scope containing the course. `item.locked`: the item is locked for you; `reasons` says what it waits for. `auth.csrf_refused`: the request did not come from this site.",
    FileUrlInvalid => "`request.invalid`: `errors` names the body's root with `item.not_a_file`: the item is not a `file` item.",
    StorageNotConfigured => "`storage.not_configured`: the server has no file storage; `detail` names `STEPCLIMB_STORAGE_URL`. `org.not_found`: the request's organization cannot be determined.",
}

/// How the browser is asked to open the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    /// Shown in the page: honored for PDFs and PNG, JPEG, GIF and WebP images; every other type
    /// is still sent as an attachment.
    Inline,
    /// Downloaded.
    Attachment,
}

/// What a signed URL is asked for.
#[derive(Clone, Copy, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FileUrlRequest {
    pub disposition: Disposition,
}

/// A short-lived URL that serves one file without a session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct SignedUrl {
    /// Absolute: the server's public URL, then `/files/` and the token. Anyone holding it can
    /// fetch the file until `expires_at`; do not share or store it.
    pub url: String,
    /// RFC 3339, UTC: 10 minutes after issue. Ask for a new URL after it.
    #[schema(format = DateTime)]
    pub expires_at: String,
    /// What the URL was issued for. An inline URL of a type that is not shown inline is still
    /// served as an attachment.
    pub disposition: Disposition,
    /// The file's name, which a download is saved under.
    pub name: String,
    /// The file's type as the server sniffed it, which the URL serves it as: what decides
    /// whether a client shows it (a PDF or an image) or offers the download only.
    pub mime: String,
    /// In bytes.
    #[schema(format = Int64)]
    pub size: u64,
}

/// What a token holds, as sealed. The names are short: the token travels in every request for
/// the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    #[serde(rename = "o")]
    pub org_id: Uuid,
    #[serde(rename = "f")]
    pub file_id: Uuid,
    #[serde(rename = "c")]
    pub course_file_id: Uuid,
    #[serde(rename = "d")]
    pub disposition: Disposition,
    /// Seconds since the Unix epoch.
    #[serde(rename = "e")]
    pub expires_at: u64,
}

impl Grant {
    /// The grant for `file` of the scope's organization, valid for [`TTL`] from `now`, counted
    /// in whole seconds.
    fn issue(
        scope: OrgScope,
        file: &course_files::CourseFile,
        disposition: Disposition,
        now: SystemTime,
    ) -> Result<Self, ApiError> {
        let since_epoch = now
            .duration_since(UNIX_EPOCH)
            .map_err(|error| ApiError::internal(&error))?;
        Ok(Self {
            org_id: scope.org_id(),
            file_id: file.file_id,
            course_file_id: file.id,
            disposition,
            expires_at: since_epoch.as_secs() + TTL.as_secs(),
        })
    }

    /// Whether the grant is still valid at `now`: strictly before `expires_at`.
    #[must_use]
    pub fn valid_at(&self, now: SystemTime) -> bool {
        now.duration_since(UNIX_EPOCH)
            .is_ok_and(|since| since.as_secs() < self.expires_at)
    }

    /// The seconds from `now` until the grant expires, 0 when it has.
    #[must_use]
    pub fn seconds_left(&self, now: SystemTime) -> u64 {
        let since = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        self.expires_at.saturating_sub(since)
    }

    fn expires_at_time(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(self.expires_at)
    }
}

/// `grant` sealed with `keys` and base64url-encoded without padding.
pub fn seal(keys: &KeyRing, grant: &Grant) -> Result<String, ApiError> {
    let plain = serde_json::to_vec(grant).map_err(|error| ApiError::internal(&error))?;
    let sealed = keys
        .encrypt(&plain, &[CONTEXT])
        .map_err(|error| ApiError::internal(&error))?;
    Ok(URL_SAFE_NO_PAD.encode(sealed))
}

/// The grant `token` seals, or `None` when it is too long, not base64url, does not open with
/// `keys` under this context (changed, sealed by another ring or for another purpose), or does
/// not hold a grant. Expiry is the caller's to check ([`Grant::valid_at`]).
#[must_use]
pub fn open(keys: &KeyRing, token: &str) -> Option<Grant> {
    if token.len() > MAX_TOKEN_CHARS {
        return None;
    }
    let sealed = URL_SAFE_NO_PAD.decode(token).ok()?;
    let plain = keys.decrypt(&sealed, &[CONTEXT]).ok()?;
    serde_json::from_slice(&plain).ok()
}

/// The answer for `file`: a URL of a fresh token, and when it expires.
pub(crate) fn signed(
    state: &AppState,
    scope: OrgScope,
    file: &course_files::CourseFile,
    disposition: Disposition,
) -> Result<SignedUrl, ApiError> {
    let grant = Grant::issue(scope, file, disposition, state.clock.now())?;
    let token = seal(&state.keys, &grant)?;
    let expires_at =
        timestamp::rfc3339(grant.expires_at_time()).map_err(|error| ApiError::internal(&error))?;
    Ok(SignedUrl {
        url: format!("{}{FILES_PATH}/{token}", state.public_url),
        expires_at,
        disposition,
        name: file.name.clone(),
        mime: file.mime.clone(),
        size: file.size,
    })
}

/// A signed URL for a `file` item's file. A caller without `course.view_all` gets it only for a
/// published item of a published module that is unlocked for them.
#[utoipa::path(
    post,
    path = "/api/v1/items/{item_id}/file-url",
    operation_id = "createItemFileUrl",
    tag = "files",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("item_id" = String, Path, format = "uuid", description = "The item's id.")),
    request_body = FileUrlRequest,
    responses(
        (status = 200, description = "A URL that serves the item's file for 10 minutes", body = SignedUrl),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = FileUrlForbidden),
        (status = 404, response = FileUrlNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = FileUrlInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = StorageNotConfigured),
    )
)]
pub async fn create_item_file_url(
    require: Require<CourseView, ItemTarget>,
    _storage: Storage,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<FileUrlRequest>,
) -> Result<Json<SignedUrl>, ApiError> {
    let target = require.target();
    let (scope, item_id, course_id) = (require.grants().scope(), target.id, target.course.id);
    let mut tx = outline::begin_read(&state).await?;
    // `require`'s course check again, in the snapshot the lock check reads: it must run before
    // `require_unlocked`, which does not check the course.
    let viewer = outline::view(&mut tx, &require, course_id, ITEM_NOT_FOUND).await?;
    if !viewer.sees_all {
        let completed = if viewer.learner {
            let user_id = require.principal().user_id;
            progress::completed_items(&mut *tx, scope, user_id, course_id)
                .await
                .map_err(progress_error)?
        } else {
            HashSet::new()
        };
        unlock::require_unlocked(&mut tx, scope, item_id, &completed, state.clock.now()).await?;
    }
    let item = items::get(&mut *tx, scope, item_id)
        .await
        .map_err(content_error)?
        .ok_or_else(|| {
            not_found_as(
                stepclimb_core::grants::Denied::CourseNotFound,
                ITEM_NOT_FOUND,
            )
        })?;
    let course_file_id = match (item.kind, item.ref_id) {
        (items::Kind::File, Some(id)) => id,
        _ => {
            return Err(ApiError::invalid(vec![FieldError::at(
                &[],
                code::NOT_A_FILE,
            )]));
        }
    };
    // `items::create` keeps a file item's course file in its course; checked again, so an item
    // that names another course's file never serves it.
    let file = Some(course_file(&mut tx, scope, course_file_id).await?)
        .filter(|file| file.course_id == course_id)
        .ok_or_else(|| {
            not_found_as(
                stepclimb_core::grants::Denied::CourseNotFound,
                FILE_NOT_FOUND,
            )
        })?;
    outline::end(tx).await?;
    Ok(Json(signed(&state, scope, &file, body.disposition)?))
}

/// A signed URL for any file of a course, for its staff (the files area and the editor).
#[utoipa::path(
    post,
    path = "/api/v1/course-files/{course_file_id}/url",
    operation_id = "createCourseFileUrl",
    tag = "files",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("course_file_id" = String, Path, format = "uuid", description = "The course file's id.")),
    request_body = FileUrlRequest,
    responses(
        (status = 200, description = "A URL that serves the file for 10 minutes", body = SignedUrl),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::course_files::FilesViewForbidden),
        (status = 404, response = crate::course_files::CourseFileNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = StorageNotConfigured),
    )
)]
pub async fn create_course_file_url(
    require: Require<CourseViewAll, CourseFileTarget>,
    _storage: Storage,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<FileUrlRequest>,
) -> Result<Json<SignedUrl>, ApiError> {
    let scope = require.grants().scope();
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(|error| ApiError::internal(&error))?;
    let file = course_file(&mut conn, scope, require.target().id).await?;
    Ok(Json(signed(&state, scope, &file, body.disposition)?))
}

/// The course file `id` of the scope's organization: 404 `file.not_found` when it is gone.
async fn course_file(
    conn: &mut sqlx::PgConnection,
    scope: OrgScope,
    id: Uuid,
) -> Result<course_files::CourseFile, ApiError> {
    course_files::get(conn, scope, id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(|| {
            not_found_as(
                stepclimb_core::grants::Denied::CourseNotFound,
                FILE_NOT_FOUND,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(id: &str) -> KeyRing {
        KeyRing::parse(&format!(
            "{id}:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8="
        ))
        .unwrap()
    }

    /// A ring with the same key id and other key bytes.
    fn other_ring() -> KeyRing {
        KeyRing::parse("test:ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8=").unwrap()
    }

    fn grant() -> Grant {
        Grant {
            org_id: Uuid::from_u128(1),
            file_id: Uuid::from_u128(2),
            course_file_id: Uuid::from_u128(3),
            disposition: Disposition::Inline,
            expires_at: 1_763_000_600,
        }
    }

    #[test]
    fn a_sealed_grant_opens_to_itself_and_is_url_safe() {
        let keys = ring("test");
        let token = seal(&keys, &grant()).unwrap();
        assert!(
            token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "{token}"
        );
        assert_eq!(open(&keys, &token), Some(grant()));
        // The grant is not readable in the token.
        assert!(!token.contains(&Uuid::from_u128(2).simple().to_string()));
    }

    #[test]
    fn a_changed_or_foreign_token_does_not_open() {
        let keys = ring("test");
        let token = seal(&keys, &grant()).unwrap();
        let mut bytes = URL_SAFE_NO_PAD.decode(&token).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert_eq!(open(&keys, &URL_SAFE_NO_PAD.encode(&bytes)), None);
        assert_eq!(open(&other_ring(), &token), None);
        assert_eq!(open(&keys, "not a token"), None);
        assert_eq!(open(&keys, &"A".repeat(MAX_TOKEN_CHARS + 1)), None);
        // Another purpose's ciphertext under the same ring.
        let elsewhere = keys
            .encrypt(&serde_json::to_vec(&grant()).unwrap(), &[b"other"])
            .unwrap();
        assert_eq!(open(&keys, &URL_SAFE_NO_PAD.encode(elsewhere)), None);
    }

    #[test]
    fn a_grant_is_valid_until_its_expiry_second() {
        let g = grant();
        let at = |s| UNIX_EPOCH + Duration::from_secs(s);
        assert!(g.valid_at(at(g.expires_at - 1)));
        assert!(!g.valid_at(at(g.expires_at)));
        assert!(!g.valid_at(at(g.expires_at + 1)));
        assert_eq!(g.seconds_left(at(g.expires_at - 600)), 600);
        assert_eq!(g.seconds_left(at(g.expires_at + 5)), 0);
    }
}
