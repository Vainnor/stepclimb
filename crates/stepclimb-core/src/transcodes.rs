// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rows a transcode changes (docs/specs/m1/08-uploaded-video.md B2, B9, B12, B13, B21, Q4).
//!
//! A `video.transcode` job runs in three short transactions around its ffmpeg work:
//!
//! 1. [`start`] locks the video, checks it is still `processing` with its whole original stored,
//!    and moves `output_run` on: the run `n` whose outputs go under `v<n>/`, so a retry never
//!    writes where a player may be reading.
//! 2. The job checks the video's minutes against `transcode_minutes`
//!    (`org_plans::check_room`) and [`charge`] records them in `transcode_usage` and on the video
//!    row (`charged_minutes`, `charged_at`), in one transaction under the plan row's lock, before
//!    ffmpeg starts (B13, owner decision 2026-11-17).
//! 3. [`fail`] or [`finish`] end it, each only while the video is `processing` and `output_run`
//!    is still `n`: a run another worker started since (a lost lease) has moved it on, and the
//!    older run's ending changes nothing. [`fail_any_run`] ends it whatever the run, for a job
//!    that stopped before it knew its run.
//!
//! The charge stays on the row until the run ends: [`finish`] and a failure the input caused
//! ([`Fault::Input`]) keep it and clear the record; a failure on the server's side
//! ([`Fault::Server`]) and [`refund_open`] give it back. A run that never ended (a crash, a lost
//! lease) leaves the record, and the next [`start`] refunds it in the transaction that moves the
//! run on, so a restart never charges a video twice.
//!
//! [`retry`] moves a video that failed as `video.transcode_failed`, with its original still
//! stored, back to `processing` for a new job; only retries after a failure the input caused
//! count toward [`MAX_RETRIES`]. [`fail_stranded`] fails, and refunds, the `processing` videos no
//! live job will finish. Every query filters by `org_id = scope.org_id()`.

use std::time::SystemTime;

use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use crate::{
    OrgScope,
    db::epoch_micros,
    videos::{self, Error, Upload},
};

/// The error code of a video whose transcode failed (B2): ffmpeg failed or broke a limit, or the
/// job ran out of attempts.
pub const TRANSCODE_FAILED: &str = "video.transcode_failed";

/// The error code of a video ffprobe could not read (B9).
pub const PROBE_FAILED: &str = "video.probe_failed";

/// The error code of a video longer than `max_video_seconds` (B9).
pub const TOO_LONG: &str = "video.too_long";

/// The error code of a video whose transcode minutes or outputs do not fit the plan (B9, B12).
pub const LIMIT_REACHED: &str = "entitlement.limit_reached";

/// The error code of a retry past [`MAX_RETRIES`] (409).
pub const RETRY_LIMIT_REACHED: &str = "video.retry_limit_reached";

/// How many times an author may retry one video's transcode (owner decision, 2026-11-17).
pub const MAX_RETRIES: i32 = 3;

/// Who caused a failure (B2, B13).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The input: ffmpeg failed on it or it broke a limit. The charge is kept and a retry counts.
    Input,
    /// The server: the store, the database or the host failed, or the job was lost. The charge is
    /// refunded and a retry does not count.
    Server,
}

/// What [`start`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// The video is gone or not `processing`: ready, failed, or still uploading. Nothing to do.
    NotProcessing,
    /// The video is `processing` at run `run` but its original is not stored whole.
    NoOriginal { run: i32 },
    /// Run `run`, from the original `upload`.
    Run { run: i32, upload: Upload },
}

/// B12, step 1: under the video's lock, if it is `processing` with a complete upload not being
/// deleted, take the next output run, stamped `now`.
pub async fn start(
    conn: &mut PgConnection,
    scope: OrgScope,
    video_id: Uuid,
    now: SystemTime,
) -> Result<Start, Error> {
    let row = sqlx::query!(
        "SELECT status, output_run FROM videos WHERE org_id = $1 AND id = $2 FOR UPDATE",
        scope.org_id(),
        video_id,
    )
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row.filter(|r| r.status == videos::Status::Processing.as_str()) else {
        return Ok(Start::NotProcessing);
    };
    // A charge still open belongs to a run that never ended: give it back before this run
    // charges its own.
    refund_open(&mut *conn, scope, video_id, None).await?;
    let upload = videos::lock_upload_of_video(&mut *conn, scope, video_id).await?;
    let Some(upload) = upload.filter(|u| u.terminating.is_none() && u.is_complete()) else {
        return Ok(Start::NoOriginal {
            run: row.output_run,
        });
    };
    let run = sqlx::query_scalar!(
        "UPDATE videos
         SET output_run = output_run + 1,
             updated_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2
         RETURNING output_run",
        scope.org_id(),
        video_id,
        epoch_micros(now)?,
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(Start::Run { run, upload })
}

/// Fail the video `video_id` with `error_code`, stamped `now`, if it is `processing` at run
/// `run`, its run's charge kept for [`Fault::Input`] and refunded for [`Fault::Server`]. Whether
/// it was changed.
pub async fn fail(
    conn: &mut PgConnection,
    scope: OrgScope,
    (video_id, run): (Uuid, i32),
    (error_code, fault): (&str, Fault),
    now: SystemTime,
) -> Result<bool, Error> {
    if fault == Fault::Server {
        refund_open(&mut *conn, scope, video_id, Some(run)).await?;
    }
    let changed = sqlx::query!(
        "UPDATE videos
         SET status = 'failed', error_code = $4, failed_by_server = $5,
             charged_minutes = NULL, charged_at = NULL,
             updated_at = timestamptz 'epoch' + $6::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2 AND output_run = $3 AND status = 'processing'",
        scope.org_id(),
        video_id,
        run,
        error_code,
        fault == Fault::Server,
        epoch_micros(now)?,
    )
    .execute(conn)
    .await?
    .rows_affected();
    Ok(changed == 1)
}

/// Fail the video `video_id` with `error_code`, stamped `now`, if it is `processing`, at any run,
/// as [`fail`] does: for a job that stopped before [`start`] told it its run. Whether it was
/// changed.
pub async fn fail_any_run(
    conn: &mut PgConnection,
    scope: OrgScope,
    video_id: Uuid,
    (error_code, fault): (&str, Fault),
    now: SystemTime,
) -> Result<bool, Error> {
    if fault == Fault::Server {
        refund_open(&mut *conn, scope, video_id, None).await?;
    }
    let changed = sqlx::query!(
        "UPDATE videos
         SET status = 'failed', error_code = $3, failed_by_server = $4,
             charged_minutes = NULL, charged_at = NULL,
             updated_at = timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2 AND status = 'processing'",
        scope.org_id(),
        video_id,
        error_code,
        fault == Fault::Server,
        epoch_micros(now)?,
    )
    .execute(conn)
    .await?
    .rows_affected();
    Ok(changed == 1)
}

/// B13: charge `minutes` to the scope's organization's month holding `now` and record them on
/// the video `video_id` at run `run`, in the caller's transaction, which has checked
/// `transcode_minutes` under the plan row's lock. Whether the video was still `processing` at
/// that run; nothing is charged otherwise.
pub async fn charge(
    conn: &mut PgConnection,
    scope: OrgScope,
    (video_id, run): (Uuid, i32),
    minutes: u64,
    now: SystemTime,
) -> Result<bool, Error> {
    let stored = i32::try_from(minutes).map_err(|_| Error::Stored {
        id: video_id,
        column: "charged_minutes",
    })?;
    let recorded = sqlx::query!(
        "UPDATE videos
         SET charged_minutes = $4,
             charged_at = timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2 AND output_run = $3 AND status = 'processing'",
        scope.org_id(),
        video_id,
        run,
        stored,
        epoch_micros(now)?,
    )
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if recorded == 1 {
        record_minutes(conn, scope, minutes, now).await?;
    }
    Ok(recorded == 1)
}

/// Give back the charge recorded on the video `video_id`, at run `run` (any run when `None`), to
/// the month it was charged in, and clear the record. The minutes refunded, if any.
pub async fn refund_open(
    conn: &mut PgConnection,
    scope: OrgScope,
    video_id: Uuid,
    run: Option<i32>,
) -> Result<Option<u64>, Error> {
    let open = sqlx::query!(
        r#"UPDATE videos v
           SET charged_minutes = NULL, charged_at = NULL
           FROM (SELECT id, charged_minutes,
                        (extract(epoch FROM charged_at) * 1000000)::bigint AS charged_at_us
                 FROM videos
                 WHERE org_id = $1 AND id = $2 AND charged_minutes IS NOT NULL
                   AND ($3::int IS NULL OR output_run = $3)
                 FOR UPDATE) old
           WHERE v.org_id = $1 AND v.id = old.id
           RETURNING old.charged_minutes AS "minutes!", old.charged_at_us AS "charged_at_us!""#,
        scope.org_id(),
        video_id,
        run,
    )
    .fetch_optional(&mut *conn)
    .await?;
    let Some(open) = open else { return Ok(None) };
    let minutes = u64::try_from(open.minutes).map_err(|_| Error::Stored {
        id: video_id,
        column: "charged_minutes",
    })?;
    let charged_at = crate::db::from_epoch_micros(open.charged_at_us)?;
    refund(conn, scope, minutes, charged_at).await?;
    Ok(Some(minutes))
}

/// What a successful run stored.
#[derive(Clone, Copy, Debug)]
pub struct Ready<'a> {
    /// Seconds.
    pub duration_s: f64,
    /// The `renditions` JSON object (B12).
    pub renditions: &'a serde_json::Value,
    /// The poster's object key.
    pub poster_key: &'a str,
    /// The bytes of every output.
    pub storage_bytes: u64,
}

/// B12: set the video `video_id` `ready` with what run `run` stored, stamped `now`, if it is
/// still `processing` at that run, keeping the run's charge ([`charge`]) and clearing its record.
/// Whether it was changed. The caller marks the upload's parts for deletion and re-checks
/// `video_storage` in the same transaction.
pub async fn finish(
    conn: &mut PgConnection,
    scope: OrgScope,
    (video_id, run): (Uuid, i32),
    ready: &Ready<'_>,
    now: SystemTime,
) -> Result<bool, Error> {
    let bytes = i64::try_from(ready.storage_bytes).map_err(|_| Error::Stored {
        id: video_id,
        column: "storage_bytes",
    })?;
    let changed = sqlx::query!(
        "UPDATE videos
         SET status = 'ready', error_code = NULL, duration_s = $4::float8, renditions = $5::text::jsonb,
             poster_key = $6, storage_bytes = $7, charged_minutes = NULL, charged_at = NULL,
             updated_at = timestamptz 'epoch' + $8::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2 AND output_run = $3 AND status = 'processing'",
        scope.org_id(),
        video_id,
        run,
        ready.duration_s,
        ready.renditions.to_string(),
        ready.poster_key,
        bytes,
        epoch_micros(now)?,
    )
    .execute(conn)
    .await?
    .rows_affected();
    Ok(changed == 1)
}

/// The whole minutes a transcode of `duration_s` seconds counts: `ceil(duration / 60)` (Q4).
#[must_use]
pub fn minutes(duration_s: f64) -> u64 {
    // A duration ffprobe gave is finite and positive; past u64 minutes it saturates.
    (duration_s / 60.0).ceil() as u64
}

/// B13: add `minutes` to the scope's organization's `transcode_usage` row of the calendar month
/// (UTC) holding `now`, creating it at the first transcode of the month. [`charge`] calls it.
pub async fn record_minutes(
    conn: &mut PgConnection,
    scope: OrgScope,
    minutes: u64,
    now: SystemTime,
) -> Result<(), Error> {
    let minutes = i32::try_from(minutes).map_err(|_| Error::Stored {
        id: scope.org_id(),
        column: "transcode_usage minutes",
    })?;
    sqlx::query!(
        "INSERT INTO transcode_usage (org_id, month, minutes, created_at, updated_at)
         VALUES ($1,
                 date_trunc('month', (timestamptz 'epoch' + $3::bigint * interval '1 microsecond')
                                     AT TIME ZONE 'UTC')::date,
                 $2,
                 timestamptz 'epoch' + $3::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $3::bigint * interval '1 microsecond')
         ON CONFLICT (org_id, month) DO UPDATE
         SET minutes = transcode_usage.minutes + EXCLUDED.minutes,
             updated_at = EXCLUDED.updated_at",
        scope.org_id(),
        minutes,
        epoch_micros(now)?,
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// B13: take `minutes` back from the scope's organization's `transcode_usage` row of the month
/// holding `charged_at`, the time [`charge`] charged them, never below 0. [`refund_open`] calls
/// it.
pub async fn refund(
    conn: &mut PgConnection,
    scope: OrgScope,
    minutes: u64,
    charged_at: SystemTime,
) -> Result<(), Error> {
    let minutes = i32::try_from(minutes).map_err(|_| Error::Stored {
        id: scope.org_id(),
        column: "transcode_usage minutes",
    })?;
    sqlx::query!(
        "UPDATE transcode_usage
         SET minutes = GREATEST(minutes - $2, 0)
         WHERE org_id = $1
           AND month = date_trunc('month', (timestamptz 'epoch' + $3::bigint
                                            * interval '1 microsecond') AT TIME ZONE 'UTC')::date",
        scope.org_id(),
        minutes,
        epoch_micros(charged_at)?,
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// The minutes the `transcode_minutes` limit counts for the scope's organization (B21): its
/// `transcode_usage` of the calendar month (UTC) holding `now`.
pub async fn minutes_for_limit(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    now: SystemTime,
) -> Result<i64, Error> {
    let minutes = sqlx::query_scalar!(
        r#"SELECT COALESCE(sum(minutes), 0)::bigint AS "minutes!"
           FROM transcode_usage
           WHERE org_id = $1
             AND month = date_trunc('month', (timestamptz 'epoch' + $2::bigint
                                              * interval '1 microsecond') AT TIME ZONE 'UTC')::date"#,
        scope.org_id(),
        epoch_micros(now)?,
    )
    .fetch_one(db)
    .await?;
    Ok(minutes)
}

/// What [`retry`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retried {
    /// The video is `processing` again; the caller enqueues its job in the same transaction.
    Processing,
    /// The video did not fail as `video.transcode_failed`, or its original is no longer stored
    /// whole.
    StatusInvalid,
    /// The video has been retried [`MAX_RETRIES`] times after failures its input caused.
    LimitReached,
    /// No such video attached to any of the courses.
    NotFound,
}

/// B2's retry: under the video's lock, move the video `video_id`, attached to one of `courses`,
/// from `failed` with [`TRANSCODE_FAILED`] back to `processing`, stamped `now`, when its upload
/// is complete and not being deleted. A video that failed on the server's side is retried freely;
/// one its input failed only fewer than [`MAX_RETRIES`] times, and the retry is counted
/// (coordinator decision, 2026-11-18).
pub async fn retry(
    conn: &mut PgConnection,
    scope: OrgScope,
    (video_id, courses): (Uuid, &[Uuid]),
    now: SystemTime,
) -> Result<Retried, Error> {
    let row = sqlx::query!(
        "SELECT status, error_code, transcode_retries, failed_by_server FROM videos v
         WHERE org_id = $1 AND id = $2
           AND EXISTS (SELECT 1 FROM course_videos cv
                       WHERE cv.org_id = $1 AND cv.video_id = $2 AND cv.course_id = ANY($3))
         FOR UPDATE",
        scope.org_id(),
        video_id,
        courses,
    )
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else {
        return Ok(Retried::NotFound);
    };
    let failed = row.status == "failed" && row.error_code.as_deref() == Some(TRANSCODE_FAILED);
    let upload = videos::lock_upload_of_video(&mut *conn, scope, video_id).await?;
    let stored = upload.is_some_and(|u| u.terminating.is_none() && u.is_complete());
    if !(failed && stored) {
        return Ok(Retried::StatusInvalid);
    }
    let counted = !row.failed_by_server;
    if counted && row.transcode_retries >= MAX_RETRIES {
        return Ok(Retried::LimitReached);
    }
    sqlx::query!(
        "UPDATE videos
         SET status = 'processing', error_code = NULL, failed_by_server = false,
             transcode_retries = transcode_retries + $3::int,
             updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        video_id,
        i32::from(counted),
        epoch_micros(now)?,
    )
    .execute(conn)
    .await?;
    Ok(Retried::Processing)
}

/// Fail with [`TRANSCODE_FAILED`] as [`Fault::Server`], stamped `now`, and refund, every
/// `processing` video of the scope's organization that no live `video.transcode` job will finish:
/// one whose job is dead (it used its attempts, or its lease lapsed on the last one), or one with
/// no job at all that has not changed since `stale_before`. A video whose job is still waiting
/// (no process that can transcode has claimed it) is left alone. The ids failed. `videos.cleanup`
/// runs it, so no video stays `processing` for good. The `jobs` table is the queue's
/// (`stepclimb-jobs`); this reads only the scope's organization's rows of one kind.
pub async fn fail_stranded(
    conn: &mut PgConnection,
    scope: OrgScope,
    stale_before: SystemTime,
    now: SystemTime,
) -> Result<Vec<Uuid>, Error> {
    let stranded = sqlx::query_scalar!(
        r#"SELECT v.id FROM videos v
           WHERE v.org_id = $1 AND v.status = 'processing'
             AND NOT EXISTS (SELECT 1 FROM jobs j
                             WHERE j.org_id = $1 AND j.kind = 'video.transcode'
                               AND j.payload->>'video_id' = v.id::text AND j.dead_at IS NULL)
             AND (EXISTS (SELECT 1 FROM jobs j
                          WHERE j.org_id = $1 AND j.kind = 'video.transcode'
                            AND j.payload->>'video_id' = v.id::text)
                  OR v.updated_at <= timestamptz 'epoch' + $2::bigint * interval '1 microsecond')
           ORDER BY v.id
           FOR UPDATE OF v SKIP LOCKED"#,
        scope.org_id(),
        epoch_micros(stale_before)?,
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut failed = Vec::with_capacity(stranded.len());
    for video_id in stranded {
        let fault = (TRANSCODE_FAILED, Fault::Server);
        if fail_any_run(&mut *conn, scope, video_id, fault, now).await? {
            failed.push(video_id);
        }
    }
    Ok(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_round_up_to_whole_minutes() {
        assert_eq!(minutes(0.5), 1);
        assert_eq!(minutes(60.0), 1);
        assert_eq!(minutes(60.001), 2);
        assert_eq!(minutes(14_400.0), 240);
    }
}
