// SPDX-License-Identifier: AGPL-3.0-or-later
//! Deleting a video and detaching it from a course (docs/specs/m1/08-uploaded-video.md B20,
//! D46, D47).
//!
//! Both run in the caller's transaction, after the caller locked every course they touch
//! (`courses::lock`), the lock `items::create` takes before it checks that a `video` item's video
//! is attached to its course. So no item can come to name the video between the check here and
//! the commit. Each then locks the video row `FOR UPDATE` and refuses a video an item still names
//! ([`Error::InUse`]) or, when the video itself would go, one being transcoded
//! ([`Error::Processing`]): its transcode would write outputs after the delete. A deleted video's
//! row takes its attachments, upload row and chapters with it (`ON DELETE CASCADE`); its stored
//! objects are the caller's to delete after the commit. Every query filters by
//! `org_id = scope.org_id()`.

use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use crate::OrgScope;

/// Why a video cannot be deleted or detached.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No video with that id in the scope's organization, or (detach) not attached to the course.
    #[error("the video does not exist")]
    VideoNotFound,
    /// An item still names the video: in any course for a delete, in the course for a detach.
    #[error("an item still uses the video")]
    InUse,
    /// The video is being transcoded and would be deleted.
    #[error("the video is being transcoded")]
    Processing,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// What [`detach`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detached {
    /// The course's attachment is gone; the video stays attached to other courses.
    Attachment,
    /// It was the video's last course: the video is deleted.
    Video,
}

/// Delete the video `video_id` of the scope's organization unless an item of any course names
/// it or it is `processing`. The caller holds the lock of every course it is attached to.
pub async fn delete(conn: &mut PgConnection, scope: OrgScope, video_id: Uuid) -> Result<(), Error> {
    let status = lock_video(conn, scope, video_id).await?;
    if in_use(conn, scope, video_id, None).await? {
        return Err(Error::InUse);
    }
    if status == "processing" {
        return Err(Error::Processing);
    }
    delete_row(conn, scope, video_id).await
}

/// Detach the video `video_id` from the course `course_id` of the scope's organization unless an
/// item of that course names it. When the course is the video's last, the video is deleted, as
/// [`delete`] would, so it is refused while `processing`. The caller holds the course's lock.
pub async fn detach(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    video_id: Uuid,
) -> Result<Detached, Error> {
    let status = lock_video(conn, scope, video_id).await?;
    let courses = sqlx::query_scalar!(
        "SELECT course_id FROM course_videos WHERE org_id = $1 AND video_id = $2",
        scope.org_id(),
        video_id,
    )
    .fetch_all(&mut *conn)
    .await?;
    if !courses.contains(&course_id) {
        return Err(Error::VideoNotFound);
    }
    if in_use(conn, scope, video_id, Some(course_id)).await? {
        return Err(Error::InUse);
    }
    if courses.len() > 1 {
        sqlx::query!(
            "DELETE FROM course_videos WHERE org_id = $1 AND course_id = $2 AND video_id = $3",
            scope.org_id(),
            course_id,
            video_id,
        )
        .execute(&mut *conn)
        .await?;
        return Ok(Detached::Attachment);
    }
    if status == "processing" {
        return Err(Error::Processing);
    }
    delete_row(conn, scope, video_id).await?;
    Ok(Detached::Video)
}

/// Whether the video `video_id` of the scope's organization exists: `videos.purge` deletes the
/// objects of a deleted video only.
pub async fn exists(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    video_id: Uuid,
) -> Result<bool, Error> {
    Ok(sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM videos WHERE org_id = $1 AND id = $2) AS "exists!""#,
        scope.org_id(),
        video_id,
    )
    .fetch_one(db)
    .await?)
}

/// The video's status, its row locked `FOR UPDATE`.
async fn lock_video(
    conn: &mut PgConnection,
    scope: OrgScope,
    video_id: Uuid,
) -> Result<String, Error> {
    sqlx::query_scalar!(
        "SELECT status FROM videos WHERE org_id = $1 AND id = $2 FOR UPDATE",
        scope.org_id(),
        video_id,
    )
    .fetch_optional(conn)
    .await?
    .ok_or(Error::VideoNotFound)
}

/// Whether a `video` item names the video, in the course `course_id` or, when `None`, anywhere.
async fn in_use(
    conn: &mut PgConnection,
    scope: OrgScope,
    video_id: Uuid,
    course_id: Option<Uuid>,
) -> Result<bool, Error> {
    Ok(sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM module_items
                          WHERE org_id = $1 AND kind = 'video' AND ref_id = $2
                            AND ($3::uuid IS NULL OR course_id = $3)) AS "used!""#,
        scope.org_id(),
        video_id,
        course_id,
    )
    .fetch_one(conn)
    .await?)
}

async fn delete_row(conn: &mut PgConnection, scope: OrgScope, video_id: Uuid) -> Result<(), Error> {
    sqlx::query!(
        "DELETE FROM videos WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        video_id,
    )
    .execute(conn)
    .await?;
    Ok(())
}
