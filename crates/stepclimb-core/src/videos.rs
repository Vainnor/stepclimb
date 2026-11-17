// SPDX-License-Identifier: AGPL-3.0-or-later
//! The videos repository (docs/specs/m1/08-uploaded-video.md B1, B2, B4 to B7, B21, Data).
//!
//! A video is one `videos` row of the organization, attached to courses through `course_videos`
//! (Q6). An uploaded video starts with a `video_uploads` row: the tus upload's length and the
//! bytes received so far, each chunk stored by the server as its own object. The bytes are the
//! server's storage layer's; this module holds rows only.
//!
//! Creating an upload locks its course row `FOR NO KEY UPDATE` and refuses an archived course
//! (spec 02 B5), as course file and module writes do. Advancing an upload's offset is one
//! statement under the upload row's lock (B6), so two chunks at the same offset cannot both
//! count. Every query filters by `org_id = scope.org_id()`.

use std::time::SystemTime;

use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    id::new_id,
    name,
};

/// The longest title, in characters (the table's check).
pub const MAX_TITLE_CHARS: usize = name::MAX_CHARS;

/// The error code of a video whose first chunk matches no video signature (B7).
pub const NOT_A_VIDEO: &str = "video.not_a_video";

/// Where a video's status stands (B2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Uploading,
    Processing,
    Ready,
    Failed,
}

impl Status {
    /// The stored and API name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uploading => "uploading",
            Self::Processing => "processing",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "uploading" => Some(Self::Uploading),
            "processing" => Some(Self::Processing),
            "ready" => Some(Self::Ready),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// A chapter mark of a video.
#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    /// Seconds from the start.
    pub at_s: f64,
    pub title: String,
}

/// A video as the repository reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Video {
    pub id: Uuid,
    /// `upload`, or one of spec 09's linked sources.
    pub source: String,
    pub title: String,
    pub status: Status,
    /// Set exactly when the status is [`Status::Failed`].
    pub error_code: Option<String>,
    /// Seconds; known once a transcode has probed the video.
    pub duration_s: Option<f64>,
    /// The heights of its HLS renditions, shortest first; empty until a transcode succeeds.
    pub heights: Vec<u32>,
    /// By `at_s`.
    pub chapters: Vec<Chapter>,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

/// A tus upload as the repository reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Upload {
    pub id: Uuid,
    pub video_id: Uuid,
    pub course_id: Uuid,
    /// The upload's whole length in bytes, from `Upload-Length`.
    pub length: u64,
    /// How many bytes are stored: the offset the next chunk starts at.
    pub received: u64,
    /// Set once its parts are being deleted: it takes no chunk and answers as gone.
    pub terminating: Option<Terminating>,
    pub updated_at: SystemTime,
}

/// What a terminating upload's delete removes (D7, D13).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terminating {
    /// The upload's parts and row only: the video stays, failed as not a video or transcoded.
    Parts,
    /// Every object of the video, transcode outputs included, then the video with its upload
    /// and attachments.
    Video,
}

impl Terminating {
    fn as_str(self) -> &'static str {
        match self {
            Self::Parts => "parts",
            Self::Video => "video",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "parts" => Some(Self::Parts),
            "video" => Some(Self::Video),
            _ => None,
        }
    }
}

impl Upload {
    /// Whether every byte has been received.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.received == self.length
    }
}

/// An upload to start.
#[derive(Clone, Copy, Debug)]
pub struct NewUpload<'a> {
    pub course_id: Uuid,
    /// Trimmed; 1 to [`MAX_TITLE_CHARS`] characters.
    pub title: &'a str,
    /// More than 0 bytes.
    pub length: u64,
    /// The user who starts it.
    pub created_by: Uuid,
}

/// Why a videos repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No course with that id in the scope's organization.
    #[error("the course does not exist")]
    CourseNotFound,
    /// The course is archived and takes no content write (spec 02 B5).
    #[error("the course is archived")]
    CourseArchived,
    /// No upload with that id in the scope's organization.
    #[error("the upload does not exist")]
    UploadNotFound,
    /// A chunk that does not start at the upload's stored offset, or that runs past its length.
    #[error("the chunk does not start at the stored offset or runs past the length")]
    OffsetMismatch,
    #[error("a title cannot be empty")]
    TitleEmpty,
    #[error("a title is longer than {MAX_TITLE_CHARS} characters")]
    TitleTooLong,
    /// A string the caller passed holds NUL, which PostgreSQL `text` cannot store.
    #[error("{field} cannot contain the NUL character")]
    ContainsNul { field: &'static str },
    /// A length of 0 or past what a `bigint` holds.
    #[error("an upload length is more than 0 and at most 2^63 - 1 bytes")]
    LengthInvalid,
    /// A stored value the table's checks keep from happening.
    #[error("the {column} of video {id} cannot be read")]
    Stored { id: Uuid, column: &'static str },
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// `title` trimmed, or why it is refused.
pub fn title(title: &str) -> Result<&str, Error> {
    name::valid(title).map_err(|invalid| match invalid {
        name::Invalid::Empty => Error::TitleEmpty,
        name::Invalid::TooLong => Error::TitleTooLong,
        name::Invalid::ContainsNul => Error::ContainsNul { field: "title" },
    })
}

/// Lock the course `id` `FOR NO KEY UPDATE` in the transaction `conn` belongs to:
/// [`Error::CourseNotFound`] when the scope's organization has no such course,
/// [`Error::CourseArchived`] when it is archived.
async fn lock_course(conn: &mut PgConnection, scope: OrgScope, id: Uuid) -> Result<(), Error> {
    let status = sqlx::query_scalar!(
        "SELECT status FROM courses WHERE org_id = $1 AND id = $2 FOR NO KEY UPDATE",
        scope.org_id(),
        id,
    )
    .fetch_optional(conn)
    .await?
    .ok_or(Error::CourseNotFound)?;
    if status == "archived" {
        return Err(Error::CourseArchived);
    }
    Ok(())
}

/// Start an upload (B4): the `videos` row (`upload`, `uploading`), attached to the course, and
/// its `video_uploads` row, stamped `now`. In the caller's transaction: the course is locked
/// until it ends. The caller checks the `video_storage` limit in the same transaction first.
pub async fn create_upload(
    conn: &mut PgConnection,
    scope: OrgScope,
    new: &NewUpload<'_>,
    now: SystemTime,
) -> Result<(Video, Upload), Error> {
    let title = title(new.title)?;
    let length = i64::try_from(new.length)
        .ok()
        .filter(|n| *n > 0)
        .ok_or(Error::LengthInvalid)?;
    lock_course(conn, scope, new.course_id).await?;
    let (video_id, upload_id, now) = (new_id(), new_id(), epoch_micros(now)?);
    sqlx::query!(
        r#"WITH video AS (
               INSERT INTO videos (id, org_id, source, title, status, created_at, updated_at)
               VALUES ($1, $3, 'upload', $5, 'uploading',
                       timestamptz 'epoch' + $8::bigint * interval '1 microsecond',
                       timestamptz 'epoch' + $8::bigint * interval '1 microsecond')
               RETURNING id),
           attached AS (
               INSERT INTO course_videos (org_id, course_id, video_id, created_at, updated_at)
               SELECT $3, $4, id,
                      timestamptz 'epoch' + $8::bigint * interval '1 microsecond',
                      timestamptz 'epoch' + $8::bigint * interval '1 microsecond'
               FROM video)
           INSERT INTO video_uploads (id, org_id, video_id, course_id, length, created_by,
                                      created_at, updated_at)
           SELECT $2, $3, id, $4, $6, $7,
                  timestamptz 'epoch' + $8::bigint * interval '1 microsecond',
                  timestamptz 'epoch' + $8::bigint * interval '1 microsecond'
           FROM video"#,
        video_id,
        upload_id,
        scope.org_id(),
        new.course_id,
        title,
        length,
        new.created_by,
        now,
    )
    .execute(&mut *conn)
    .await?;
    let video = get(&mut *conn, scope, video_id)
        .await?
        .ok_or(Error::Stored {
            id: video_id,
            column: "row",
        })?;
    let upload = get_upload(&mut *conn, scope, upload_id)
        .await?
        .ok_or(Error::UploadNotFound)?;
    Ok((video, upload))
}

struct UploadRow {
    id: Uuid,
    video_id: Uuid,
    course_id: Uuid,
    length: i64,
    received: i64,
    terminating: Option<String>,
    updated_at_us: i64,
}

impl TryFrom<UploadRow> for Upload {
    type Error = Error;

    fn try_from(row: UploadRow) -> Result<Self, Error> {
        let stored = |column| Error::Stored {
            id: row.video_id,
            column,
        };
        Ok(Self {
            length: u64::try_from(row.length).map_err(|_| stored("upload length"))?,
            received: u64::try_from(row.received).map_err(|_| stored("upload offset"))?,
            updated_at: from_epoch_micros(row.updated_at_us)?,
            terminating: row
                .terminating
                .as_deref()
                .map(|value| Terminating::parse(value).ok_or_else(|| stored("terminating")))
                .transpose()?,
            id: row.id,
            video_id: row.video_id,
            course_id: row.course_id,
        })
    }
}

/// The upload `id` of the scope's organization, or `None`.
pub async fn get_upload(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Upload>, Error> {
    sqlx::query_as!(
        UploadRow,
        r#"SELECT id, video_id, course_id, length, received, terminating,
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM video_uploads WHERE org_id = $1 AND id = $2"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?
    .map(Upload::try_from)
    .transpose()
}

/// The upload `id` locked `FOR UPDATE` in the transaction `conn` belongs to, or `None`. A chunk,
/// a termination and `videos.cleanup` each hold it while they change the upload or its parts.
pub async fn lock_upload(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Upload>, Error> {
    sqlx::query_as!(
        UploadRow,
        r#"SELECT id, video_id, course_id, length, received, terminating,
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM video_uploads WHERE org_id = $1 AND id = $2 FOR UPDATE"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(conn)
    .await?
    .map(Upload::try_from)
    .transpose()
}

/// The upload of the video `video_id` locked `FOR UPDATE` in the transaction `conn` belongs to,
/// or `None` when it has none: a transcode reads the original it concatenates from it.
pub async fn lock_upload_of_video(
    conn: &mut PgConnection,
    scope: OrgScope,
    video_id: Uuid,
) -> Result<Option<Upload>, Error> {
    sqlx::query_as!(
        UploadRow,
        r#"SELECT id, video_id, course_id, length, received, terminating,
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM video_uploads WHERE org_id = $1 AND video_id = $2 FOR UPDATE"#,
        scope.org_id(),
        video_id,
    )
    .fetch_optional(conn)
    .await?
    .map(Upload::try_from)
    .transpose()
}

/// The course of the upload `id` of the scope's organization, or `None`: what the upload routes'
/// target checks grants at.
pub async fn upload_course(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Uuid>, Error> {
    let course = sqlx::query_scalar!(
        "SELECT course_id FROM video_uploads WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?;
    Ok(course)
}

/// Add `chunk` bytes to the upload `id` whose stored offset is `from` (B5, B6), stamped `now`,
/// in one statement: [`Error::OffsetMismatch`] when the offset is no longer `from` or the chunk
/// would run past the length. The caller holds the row's lock ([`lock_upload`]).
pub async fn advance(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
    (from, chunk): (u64, u64),
    now: SystemTime,
) -> Result<Upload, Error> {
    let (Ok(from), Ok(chunk)) = (i64::try_from(from), i64::try_from(chunk)) else {
        return Err(Error::OffsetMismatch);
    };
    sqlx::query_as!(
        UploadRow,
        r#"UPDATE video_uploads
           SET received = received + $4,
               updated_at = timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
           WHERE org_id = $1 AND id = $2 AND received = $3 AND received + $4 <= length
             AND terminating IS NULL
           RETURNING id, video_id, course_id, length, received, terminating,
                     (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!""#,
        scope.org_id(),
        id,
        from,
        chunk,
        epoch_micros(now)?,
    )
    .fetch_optional(conn)
    .await?
    .ok_or(Error::OffsetMismatch)?
    .try_into()
}

/// Move the video `id` from `uploading` to `processing` (B7), or to `failed` with `error_code`
/// when `error_code` is set, stamped `now`. Whether a video in another status was changed: a
/// video already past `uploading` stays as it is.
pub async fn finish_upload(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
    error_code: Option<&str>,
    now: SystemTime,
) -> Result<bool, Error> {
    let status = match error_code {
        Some(_) => Status::Failed,
        None => Status::Processing,
    };
    let changed = sqlx::query!(
        "UPDATE videos
         SET status = $3, error_code = $4,
             updated_at = timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2 AND status = 'uploading'",
        scope.org_id(),
        id,
        status.as_str(),
        error_code,
        epoch_micros(now)?,
    )
    .execute(conn)
    .await?
    .rows_affected();
    Ok(changed == 1)
}

/// Mark the upload `id` as terminating, `what` saying what goes after its parts, stamped `now`
/// (D13). From the commit on, [`advance`] refuses it and its parts can be deleted without its
/// row's lock: no chunk is stored for it any more. The caller holds the row's lock
/// ([`lock_upload`]). Whether a row that was not terminating was marked.
pub async fn mark_terminating(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
    what: Terminating,
    now: SystemTime,
) -> Result<bool, Error> {
    let marked = sqlx::query!(
        "UPDATE video_uploads
         SET terminating = $3,
             updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2 AND terminating IS NULL",
        scope.org_id(),
        id,
        what.as_str(),
        epoch_micros(now)?,
    )
    .execute(conn)
    .await?
    .rows_affected();
    Ok(marked == 1)
}

/// End the terminating upload `upload`, whose parts the caller has deleted: its row, and for
/// [`Terminating::Video`] its video with the video's attachments and chapters (the foreign keys
/// cascade). Whether a row was deleted; an upload that is not terminating is left alone.
pub async fn finish_termination(
    conn: &mut PgConnection,
    scope: OrgScope,
    upload: &Upload,
) -> Result<bool, Error> {
    let deleted = match upload.terminating {
        None => return Ok(false),
        Some(Terminating::Parts) => sqlx::query!(
            "DELETE FROM video_uploads WHERE org_id = $1 AND id = $2 AND terminating = 'parts'",
            scope.org_id(),
            upload.id,
        )
        .execute(conn)
        .await?
        .rows_affected(),
        Some(Terminating::Video) => sqlx::query!(
            "DELETE FROM videos WHERE org_id = $1 AND id = $2",
            scope.org_id(),
            upload.video_id,
        )
        .execute(conn)
        .await?
        .rows_affected(),
    };
    Ok(deleted == 1)
}

/// The scope's organization's terminating uploads (D13): those whose parts a delete has not
/// finished removing.
pub async fn terminating_uploads(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
) -> Result<Vec<Upload>, Error> {
    sqlx::query_as!(
        UploadRow,
        r#"SELECT id, video_id, course_id, length, received, terminating,
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM video_uploads WHERE org_id = $1 AND terminating IS NOT NULL
           ORDER BY id"#,
        scope.org_id(),
    )
    .fetch_all(db)
    .await?
    .into_iter()
    .map(Upload::try_from)
    .collect()
}

/// The scope's organization's incomplete uploads, not terminating, last changed at or before
/// `cutoff`, whose video is still `uploading` (B6): what `videos.cleanup` deletes.
pub async fn stale_uploads(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    cutoff: SystemTime,
) -> Result<Vec<Upload>, Error> {
    sqlx::query_as!(
        UploadRow,
        r#"SELECT u.id, u.video_id, u.course_id, u.length, u.received, u.terminating,
                  (extract(epoch FROM u.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM video_uploads u JOIN videos v ON v.org_id = u.org_id AND v.id = u.video_id
           WHERE u.org_id = $1 AND v.status = 'uploading' AND u.received < u.length
             AND u.terminating IS NULL
             AND u.updated_at <= timestamptz 'epoch' + $2::bigint * interval '1 microsecond'
           ORDER BY u.id"#,
        scope.org_id(),
        epoch_micros(cutoff)?,
    )
    .fetch_all(db)
    .await?
    .into_iter()
    .map(Upload::try_from)
    .collect()
}

/// The scope's organization's uploads, not yet terminating, whose video failed as
/// [`NOT_A_VIDEO`]: parts nothing will read. A video that failed for another reason keeps its
/// original for a retry (B2), so its upload is not listed.
pub async fn not_a_video_uploads(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
) -> Result<Vec<Upload>, Error> {
    sqlx::query_as!(
        UploadRow,
        r#"SELECT u.id, u.video_id, u.course_id, u.length, u.received, u.terminating,
                  (extract(epoch FROM u.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM video_uploads u JOIN videos v ON v.org_id = u.org_id AND v.id = u.video_id
           WHERE u.org_id = $1 AND v.status = 'failed' AND v.error_code = $2
             AND u.terminating IS NULL
           ORDER BY u.id"#,
        scope.org_id(),
        NOT_A_VIDEO,
    )
    .fetch_all(db)
    .await?
    .into_iter()
    .map(Upload::try_from)
    .collect()
}

/// The ids of the courses the video `id` is attached to, in id order; empty when the scope's
/// organization has no such video.
pub async fn courses_of(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Vec<Uuid>, Error> {
    let courses = sqlx::query_scalar!(
        "SELECT course_id FROM course_videos WHERE org_id = $1 AND video_id = $2
         ORDER BY course_id",
        scope.org_id(),
        id,
    )
    .fetch_all(db)
    .await?;
    Ok(courses)
}

struct VideoRow {
    id: Uuid,
    source: String,
    title: String,
    status: String,
    error_code: Option<String>,
    duration_s: Option<f64>,
    heights: Vec<i32>,
    created_at_us: i64,
    updated_at_us: i64,
}

impl VideoRow {
    fn into_video(self, chapters: Vec<Chapter>) -> Result<Video, Error> {
        Ok(Video {
            status: Status::parse(&self.status).ok_or(Error::Stored {
                id: self.id,
                column: "status",
            })?,
            created_at: from_epoch_micros(self.created_at_us)?,
            updated_at: from_epoch_micros(self.updated_at_us)?,
            id: self.id,
            source: self.source,
            title: self.title,
            error_code: self.error_code,
            duration_s: self.duration_s,
            heights: self
                .heights
                .into_iter()
                .map(u32::try_from)
                .collect::<Result<_, _>>()
                .map_err(|_| Error::Stored {
                    id: self.id,
                    column: "renditions",
                })?,
            chapters,
        })
    }
}

/// The video `id` of the scope's organization with its chapters, or `None`.
pub async fn get(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Video>, Error> {
    let row = sqlx::query_as!(
        VideoRow,
        r#"SELECT id, source, title, status, error_code, duration_s::float8 AS duration_s,
                  ARRAY(SELECT h::int FROM jsonb_array_elements_text(
                            COALESCE(renditions->'heights', '[]'::jsonb)) AS h) AS "heights!",
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM videos WHERE org_id = $1 AND id = $2"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let mut chapters = chapters_of(&mut *conn, scope, &[id]).await?;
    row.into_video(chapters.remove(&id).unwrap_or_default())
        .map(Some)
}

/// One page of the videos attached to the course `course_id`, by id, after `after`, at most
/// `limit` (the caller passes one more than the page to learn whether another follows).
pub async fn list_for_course(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    after: Option<Uuid>,
    limit: i64,
) -> Result<Vec<Video>, Error> {
    let rows = sqlx::query_as!(
        VideoRow,
        r#"SELECT v.id, v.source, v.title, v.status, v.error_code,
                  v.duration_s::float8 AS duration_s,
                  ARRAY(SELECT h::int FROM jsonb_array_elements_text(
                            COALESCE(v.renditions->'heights', '[]'::jsonb)) AS h) AS "heights!",
                  (extract(epoch FROM v.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM v.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM course_videos cv JOIN videos v ON v.org_id = cv.org_id AND v.id = cv.video_id
           WHERE cv.org_id = $1 AND cv.course_id = $2 AND ($3::uuid IS NULL OR v.id > $3)
           ORDER BY v.id
           LIMIT $4"#,
        scope.org_id(),
        course_id,
        after,
        limit,
    )
    .fetch_all(&mut *conn)
    .await?;
    let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let mut chapters = chapters_of(&mut *conn, scope, &ids).await?;
    rows.into_iter()
        .map(|row| {
            let of = chapters.remove(&row.id).unwrap_or_default();
            row.into_video(of)
        })
        .collect()
}

/// The chapters of the videos `ids`, each list by `at_s`.
async fn chapters_of(
    conn: &mut PgConnection,
    scope: OrgScope,
    ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, Vec<Chapter>>, Error> {
    let rows = sqlx::query!(
        r#"SELECT video_id, at_s::float8 AS "at_s!", title
           FROM video_chapters WHERE org_id = $1 AND video_id = ANY($2)
           ORDER BY video_id, at_s"#,
        scope.org_id(),
        ids,
    )
    .fetch_all(conn)
    .await?;
    let mut chapters = std::collections::HashMap::<Uuid, Vec<Chapter>>::new();
    for row in rows {
        chapters.entry(row.video_id).or_default().push(Chapter {
            at_s: row.at_s,
            title: row.title,
        });
    }
    Ok(chapters)
}

/// Set the title of the video `id`, stamped `now`, and read it back; `None` when the scope's
/// organization has no such video attached to any of `courses`, the courses the caller's check
/// passed at.
pub async fn rename(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
    (courses, new_title): (&[Uuid], &str),
    now: SystemTime,
) -> Result<Option<Video>, Error> {
    let new_title = title(new_title)?;
    let renamed = sqlx::query!(
        "UPDATE videos
         SET title = $3,
             updated_at = timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2
           AND EXISTS (SELECT 1 FROM course_videos cv
                       WHERE cv.org_id = $1 AND cv.video_id = $2 AND cv.course_id = ANY($4))",
        scope.org_id(),
        id,
        new_title,
        courses,
        epoch_micros(now)?,
    )
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if renamed == 0 {
        return Ok(None);
    }
    get(conn, scope, id).await
}

/// The bytes the `video_storage` limit counts for the scope's organization (B21): the stored
/// renditions of its videos, plus the length of every upload whose parts are still stored. An
/// upload counts its whole length from its start, so a started upload always has room to finish.
/// The upload of a `ready` video is not counted: its parts are being deleted (Q3), and the
/// video's renditions count in their place.
pub async fn count_for_limit(db: impl PgExecutor<'_>, scope: OrgScope) -> Result<i64, Error> {
    let bytes = sqlx::query_scalar!(
        r#"SELECT ((SELECT COALESCE(sum(storage_bytes), 0) FROM videos WHERE org_id = $1)
                 + (SELECT COALESCE(sum(u.length), 0)
                    FROM video_uploads u JOIN videos v ON v.org_id = u.org_id AND v.id = u.video_id
                    WHERE u.org_id = $1 AND v.status <> 'ready')
                )::bigint AS "bytes!""#,
        scope.org_id(),
    )
    .fetch_one(db)
    .await?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_round_trips_through_its_stored_spelling() {
        for status in [
            Status::Uploading,
            Status::Processing,
            Status::Ready,
            Status::Failed,
        ] {
            assert_eq!(Status::parse(status.as_str()), Some(status));
        }
        assert_eq!(Status::parse("Ready"), None);
    }

    #[test]
    fn titles_are_trimmed_and_bounded() {
        assert_eq!(title("  Week 1 lecture ").unwrap(), "Week 1 lecture");
        assert!(matches!(title(" "), Err(Error::TitleEmpty)));
        let over = "a".repeat(MAX_TITLE_CHARS + 1);
        assert!(matches!(title(&over), Err(Error::TitleTooLong)));
        assert!(matches!(
            title("a\0b"),
            Err(Error::ContainsNul { field: "title" })
        ));
    }

    #[test]
    fn every_termination_round_trips_through_its_stored_spelling() {
        for what in [Terminating::Parts, Terminating::Video] {
            assert_eq!(Terminating::parse(what.as_str()), Some(what));
        }
        assert_eq!(Terminating::parse("all"), None);
    }

    #[test]
    fn an_upload_is_complete_at_its_length() {
        let upload = |received| Upload {
            id: Uuid::nil(),
            video_id: Uuid::nil(),
            course_id: Uuid::nil(),
            length: 10,
            received,
            terminating: None,
            updated_at: SystemTime::UNIX_EPOCH,
        };
        assert!(!upload(9).is_complete());
        assert!(upload(10).is_complete());
    }
}
