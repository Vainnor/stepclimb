// SPDX-License-Identifier: AGPL-3.0-or-later
//! A course's files area (docs/specs/m1/06-files.md B9 to B12 and API): list a folder, read,
//! rename or move a course file, rename a folder, and delete a course file. Uploading is
//! [`crate::upload`]; this module holds the schemas and the error mapping both use.
//!
//! The reads take `Require<perm::CourseViewAll, _>` (Q5: learners reach files only through
//! `file` items); the writes take `Require<perm::CourseContentEdit, _>` at the course, directly or
//! through the course file ([`CourseFileTarget`]), and check it again under the course lock as
//! [`crate::modules`] does. None of these routes reads stored bytes, so none needs storage
//! (B1a).

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use stepclimb_core::{
    course_files::{self, Error, Folder, Name, PathError},
    grants::Denied,
    id::Uuid,
    perm::{CourseContentEdit, CourseViewAll},
};
use utoipa::{IntoParams, ToSchema};

use crate::{
    AppState,
    error::{ApiError, FieldError},
    extract::{ApiJson, ApiQuery},
    modules::{begin, commit, recheck},
    openapi::problem_responses,
    organization::present,
    pagination::{Page, PageParams},
    require::{
        CourseFileTarget, CourseTarget, Require,
        code::{COURSE_NOT_FOUND, FILE_NOT_FOUND},
        not_found_as,
    },
    timestamp,
};

/// The path of a course file; `{COURSE_FILES_PATH}/{course_file_id}`.
pub const COURSE_FILES_PATH: &str = "/api/v1/course-files";

/// The codes this module and [`crate::upload`] produce besides the targets' 404s.
pub mod code {
    /// The folder already holds a file with the name (409).
    pub const NAME_TAKEN: &str = "file.name_taken";
    /// A `file` item references the course file (409).
    pub const IN_USE: &str = "file.in_use";
    /// No course file is in the folder or below it (404).
    pub const FOLDER_NOT_FOUND: &str = "folder.not_found";
    /// A folder that breaks B9's rules (a 422 field code).
    pub const FOLDER_INVALID: &str = "file.folder_invalid";
    /// A folder segment over 100 characters (a 422 field code).
    pub const FOLDER_TOO_LONG: &str = "file.folder_too_long";
    /// A folder more than 8 levels deep (a 422 field code).
    pub const FOLDER_TOO_DEEP: &str = "file.folder_too_deep";
    /// The top folder `/` named where a folder rename needs another (a 422 field code).
    pub const FOLDER_ROOT: &str = "file.folder_root";
    /// A name that breaks B9's rules (a 422 field code).
    pub const NAME_INVALID: &str = "file.name_invalid";
    /// A name over 255 characters (a 422 field code).
    pub const NAME_TOO_LONG: &str = "file.name_too_long";
}

problem_responses! {
    CourseFileNotFound => "`file.not_found`: no course file with this id exists in the organization, or you cannot see its course. `org.not_found`: the request's organization cannot be determined.",
    FilesCourseNotFound => "`course.not_found`: no course with this id exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined.",
    FolderRenameNotFound => "`course.not_found`: no course with this id exists in the organization, or you cannot see it. `folder.not_found`: no file is in the folder `from` or below it. `org.not_found`: the request's organization cannot be determined.",
    FilesViewForbidden => "`permission.denied`: you can see the course but do not hold `course.view_all` at a scope containing it.",
    CourseFileWriteConflict => "`course.archived`: the course is archived and takes no content change. `file.name_taken`: the folder already holds a file with this name.",
    CourseFileDeleteConflict => "`course.archived`: the course is archived and takes no content change. `file.in_use`: a `file` item references the course file; remove it first.",
    CourseFileInvalid => "`request.invalid`: `errors` names `/folder` (`file.folder_invalid`: not `/` or a path of `/`-separated segments that are not empty, `.` or `..` and hold no `\\` or control character; `file.folder_too_long`: a segment over 100 characters; `file.folder_too_deep`: more than 8 segments) or `/name` (`file.name_invalid`: empty, `.`, `..`, or holding `/`, `\\` or a control character; `file.name_too_long`: over 255 characters). Both are compared after NFC normalization.",
    FolderRenameInvalid => "`request.invalid`: `errors` names `/from` or `/to` with `file.folder_invalid`, `file.folder_too_long` or `file.folder_too_deep` as for a course file's folder, `file.folder_root` (the top folder `/` cannot be renamed or be the new name), or `/to` with `file.folder_too_deep` when a moved file would be more than 8 levels deep.",
}

/// A file in a folder of a course.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct CourseFile {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "uuid")]
    pub course_id: Uuid,
    /// `/` or a path such as `/handouts/week-1`, NFC-normalized.
    pub folder: String,
    /// NFC-normalized; unique in the folder.
    pub name: String,
    /// In bytes, as the server counted them.
    #[schema(format = Int64)]
    pub size: u64,
    /// The type the server read from the file's first bytes; `application/octet-stream` when it
    /// recognized none. Never the uploader's `Content-Type`.
    pub mime: String,
    /// The SHA-256 of the bytes as the server computed it, 64 lowercase hexadecimal digits.
    pub sha256: String,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub updated_at: String,
}

impl CourseFile {
    pub(crate) fn from_repo(file: course_files::CourseFile) -> Result<Self, ApiError> {
        let time = |t| timestamp::rfc3339(t).map_err(|error| ApiError::internal(&error));
        Ok(Self {
            created_at: time(file.created_at)?,
            updated_at: time(file.updated_at)?,
            sha256: file.sha256.hex(),
            id: file.id,
            course_id: file.course_id,
            folder: file.folder,
            name: file.name,
            size: file.size,
            mime: file.mime,
        })
    }
}

/// One entry of a folder: a folder directly inside it, or a file in it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FolderEntry {
    /// A folder that holds files, directly or below it.
    Folder {
        /// The folder's whole path, such as `/handouts/week-1`.
        path: String,
        /// Its last segment.
        name: String,
    },
    File {
        file: CourseFile,
    },
}

/// Which folder to list.
#[derive(Clone, Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct FolderParams {
    /// The folder, such as `/handouts`; `/`, the course's top folder, when omitted.
    pub folder: Option<String>,
}

/// A change to a course file; an omitted member is left as it is.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CourseFilePatch {
    /// The folder to move the file to, in the same course.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub folder: Option<String>,
    /// The file's new name.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub name: Option<String>,
}

/// A folder to rename, with everything below it.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FolderRename {
    /// The folder's path now; not `/`.
    pub from: String,
    /// Its new path; not `/`.
    pub to: String,
}

/// How many course files a folder rename moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct FolderRenamed {
    pub moved: u64,
}

/// `value` as a folder, or the 422 that names `field`.
pub(crate) fn folder(field: &'static str, value: &str) -> Result<Folder, ApiError> {
    Folder::parse(value).map_err(|error| {
        let code = match error {
            PathError::Invalid => code::FOLDER_INVALID,
            PathError::TooLong => code::FOLDER_TOO_LONG,
            PathError::TooDeep => code::FOLDER_TOO_DEEP,
        };
        ApiError::invalid(vec![FieldError::at(&[field], code)])
    })
}

/// `value` as a name, or the 422 that names `/name`.
pub(crate) fn name(value: &str) -> Result<Name, ApiError> {
    Name::parse(value).map_err(|error| {
        let code = match error {
            PathError::TooLong => code::NAME_TOO_LONG,
            PathError::Invalid | PathError::TooDeep => code::NAME_INVALID,
        };
        ApiError::invalid(vec![FieldError::at(&["name"], code)])
    })
}

/// The answer for a course files repository error.
pub(crate) fn file_error(error: Error) -> ApiError {
    let conflict = |code, detail| ApiError::new(StatusCode::CONFLICT, code, detail);
    match error {
        Error::CourseNotFound => not_found_as(Denied::CourseNotFound, COURSE_NOT_FOUND),
        Error::NotFound => not_found_as(Denied::CourseNotFound, FILE_NOT_FOUND),
        Error::FolderNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            code::FOLDER_NOT_FOUND,
            "No file is in this folder or below it.",
        ),
        Error::CourseArchived => conflict(
            crate::modules::code::COURSE_ARCHIVED,
            "The course is archived and takes no content change.",
        ),
        Error::NameTaken => conflict(
            code::NAME_TAKEN,
            "The folder already holds a file with this name.",
        ),
        Error::InUse => conflict(
            code::IN_USE,
            "A file item references this course file; remove it first.",
        ),
        Error::TooDeep => ApiError::invalid(vec![FieldError::at(&["to"], code::FOLDER_TOO_DEEP)]),
        Error::RootFolder => ApiError::invalid(vec![FieldError::at(&["from"], code::FOLDER_ROOT)]),
        other @ (Error::File(_) | Error::Time(_) | Error::Database(_)) => {
            ApiError::internal(&other)
        }
    }
}

/// List the folders and files directly in a folder of the course: folders first, then files,
/// each sorted by name (compared as bytes).
#[utoipa::path(
    get,
    path = "/api/v1/courses/{course_id}/files",
    operation_id = "listCourseFiles",
    tag = "files",
    params(
        ("course_id" = String, Path, format = "uuid", description = "The course's id."),
        FolderParams,
        PageParams,
    ),
    responses(
        (status = 200, description = "One page of the folder's entries", body = Page<FolderEntry>),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = FilesViewForbidden),
        (status = 404, response = FilesCourseNotFound),
        (status = 422, response = CourseFileInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_course_files(
    require: Require<CourseViewAll, CourseTarget>,
    State(state): State<AppState>,
    ApiQuery(params): ApiQuery<FolderParams>,
    ApiQuery(page): ApiQuery<PageParams>,
) -> Result<Json<Page<FolderEntry>>, ApiError> {
    let page = page.parse::<course_files::ListKey>()?;
    let at = match &params.folder {
        Some(value) => folder("folder", value)?,
        None => Folder::root(),
    };
    let scope = require.grants().scope();
    let (after, limit) = (page.after(), page.fetch_limit());
    // The cursor's name is the one caller's string the folder rules have not checked, and
    // PostgreSQL text cannot hold NUL.
    if after.is_some_and(|(_, name)| name.contains('\0')) {
        return Err(ApiError::invalid_cursor());
    }
    let entries =
        course_files::list_folder(&state.db, scope, require.target().id, &at, after, limit)
            .await
            .map_err(file_error)?;
    let keys: Vec<course_files::ListKey> = entries.iter().map(course_files::Entry::key).collect();
    let rows = entries
        .into_iter()
        .map(|entry| match entry {
            course_files::Entry::Folder { name } => Ok(FolderEntry::Folder {
                path: at.child(&name),
                name,
            }),
            course_files::Entry::File(file) => {
                CourseFile::from_repo(file).map(|file| FolderEntry::File { file })
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let rows: Vec<(FolderEntry, course_files::ListKey)> = rows.into_iter().zip(keys).collect();
    let page = page.page(rows, |(_, key)| key.clone())?;
    Ok(Json(Page {
        items: page.items.into_iter().map(|(entry, _)| entry).collect(),
        next_cursor: page.next_cursor,
    }))
}

/// Read a course file.
#[utoipa::path(
    get,
    path = "/api/v1/course-files/{course_file_id}",
    operation_id = "getCourseFile",
    tag = "files",
    params(("course_file_id" = String, Path, format = "uuid", description = "The course file's id.")),
    responses(
        (status = 200, description = "The course file", body = CourseFile),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = FilesViewForbidden),
        (status = 404, response = CourseFileNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_course_file(
    require: Require<CourseViewAll, CourseFileTarget>,
    State(state): State<AppState>,
) -> Result<Json<CourseFile>, ApiError> {
    let file = course_files::get(&state.db, require.grants().scope(), require.target().id)
        .await
        .map_err(file_error)?
        .ok_or_else(|| file_error(Error::NotFound))?;
    Ok(Json(CourseFile::from_repo(file)?))
}

/// Rename a course file, or move it to another folder of its course. A patch naming only current
/// values changes nothing.
#[utoipa::path(
    patch,
    path = "/api/v1/course-files/{course_file_id}",
    operation_id = "updateCourseFile",
    tag = "files",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("course_file_id" = String, Path, format = "uuid", description = "The course file's id.")),
    request_body = CourseFilePatch,
    responses(
        (status = 200, description = "The course file after the change", body = CourseFile),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::modules::ContentEditForbidden),
        (status = 404, response = CourseFileNotFound),
        (status = 409, response = CourseFileWriteConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = CourseFileInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_course_file(
    require: Require<CourseContentEdit, CourseFileTarget>,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<CourseFilePatch>,
) -> Result<Json<CourseFile>, ApiError> {
    let to_folder = body
        .folder
        .as_deref()
        .map(|f| folder("folder", f))
        .transpose()?;
    let to_name = body.name.as_deref().map(name).transpose()?;
    let target = require.target();
    let mut tx = begin(&state).await?;
    recheck(&mut tx, &require, target.course.id, FILE_NOT_FOUND).await?;
    let change = (to_folder.as_ref(), to_name.as_ref());
    let file = course_files::update(
        &mut tx,
        require.grants().scope(),
        target.id,
        change,
        state.clock.now(),
    )
    .await
    .map_err(file_error)?;
    commit(tx).await?;
    Ok(Json(CourseFile::from_repo(file)?))
}

/// Delete a course file no `file` item references. Its bytes stay stored until no course file
/// has used them for 24 hours.
#[utoipa::path(
    delete,
    path = "/api/v1/course-files/{course_file_id}",
    operation_id = "deleteCourseFile",
    tag = "files",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("course_file_id" = String, Path, format = "uuid", description = "The course file's id.")),
    responses(
        (status = 204, description = "The course file is deleted"),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::modules::ContentEditForbidden),
        (status = 404, response = CourseFileNotFound),
        (status = 409, response = CourseFileDeleteConflict),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn delete_course_file(
    require: Require<CourseContentEdit, CourseFileTarget>,
    State(state): State<AppState>,
) -> Result<StatusCode, ApiError> {
    let target = require.target();
    let mut tx = begin(&state).await?;
    recheck(&mut tx, &require, target.course.id, FILE_NOT_FOUND).await?;
    course_files::delete(
        &mut tx,
        require.grants().scope(),
        target.id,
        state.clock.now(),
    )
    .await
    .map_err(file_error)?;
    commit(tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Rename a folder of the course and every folder below it, in one change: either every file
/// moves or none does.
#[utoipa::path(
    post,
    path = "/api/v1/courses/{course_id}/folders/rename",
    operation_id = "renameCourseFolder",
    tag = "files",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("course_id" = String, Path, format = "uuid", description = "The course's id.")),
    request_body = FolderRename,
    responses(
        (status = 200, description = "How many course files moved", body = FolderRenamed),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::modules::ContentEditForbidden),
        (status = 404, response = FolderRenameNotFound),
        (status = 409, response = CourseFileWriteConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = FolderRenameInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn rename_course_folder(
    require: Require<CourseContentEdit, CourseTarget>,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<FolderRename>,
) -> Result<Json<FolderRenamed>, ApiError> {
    let (from, to) = (folder("from", &body.from)?, folder("to", &body.to)?);
    for (field, path) in [("from", &from), ("to", &to)] {
        if path.is_root() {
            return Err(ApiError::invalid(vec![FieldError::at(
                &[field],
                code::FOLDER_ROOT,
            )]));
        }
    }
    let course_id = require.target().id;
    let mut tx = begin(&state).await?;
    recheck(&mut tx, &require, course_id, COURSE_NOT_FOUND).await?;
    let moved = course_files::rename_folder(
        &mut tx,
        require.grants().scope(),
        course_id,
        (&from, &to),
        state.clock.now(),
    )
    .await
    .map_err(file_error)?;
    commit(tx).await?;
    Ok(Json(FolderRenamed { moved }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn repository_errors_map_to_their_status_code_and_field() {
        let cases = [
            (Error::CourseNotFound, 404, "course.not_found", None),
            (Error::NotFound, 404, "file.not_found", None),
            (Error::FolderNotFound, 404, "folder.not_found", None),
            (Error::CourseArchived, 409, "course.archived", None),
            (Error::NameTaken, 409, "file.name_taken", None),
            (Error::InUse, 409, "file.in_use", None),
            (
                Error::TooDeep,
                422,
                "request.invalid",
                Some(("/to", "file.folder_too_deep")),
            ),
            (
                Error::RootFolder,
                422,
                "request.invalid",
                Some(("/from", "file.folder_root")),
            ),
        ];
        for (error, status, code, field) in cases {
            let problem = file_error(error).problem();
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
    fn path_rules_answer_their_field_codes() {
        let code = |e: ApiError| e.problem().errors[0].clone();
        assert_eq!(
            code(folder("folder", "/a/..").unwrap_err()),
            FieldError::at(&["folder"], "file.folder_invalid")
        );
        assert_eq!(
            code(folder("to", "/1/2/3/4/5/6/7/8/9").unwrap_err()),
            FieldError::at(&["to"], "file.folder_too_deep")
        );
        assert_eq!(
            code(name(&"n".repeat(256)).unwrap_err()),
            FieldError::at(&["name"], "file.name_too_long")
        );
        assert_eq!(
            code(name("a/b").unwrap_err()),
            FieldError::at(&["name"], "file.name_invalid")
        );
    }

    #[test]
    fn a_patch_refuses_null_and_unknown_members() {
        for refused in [
            json!({ "folder": null }),
            json!({ "name": null }),
            json!({ "course_id": "x" }),
        ] {
            assert!(
                serde_json::from_value::<CourseFilePatch>(refused.clone()).is_err(),
                "{refused}"
            );
        }
    }

    #[test]
    fn entries_carry_their_kind() {
        let folder = FolderEntry::Folder {
            path: "/a/b".into(),
            name: "b".into(),
        };
        assert_eq!(
            serde_json::to_value(folder).unwrap(),
            json!({ "kind": "folder", "path": "/a/b", "name": "b" })
        );
    }
}
