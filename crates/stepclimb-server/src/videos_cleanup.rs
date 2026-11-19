// SPDX-License-Identifier: AGPL-3.0-or-later
//! `videos.cleanup` (docs/specs/m1/08-uploaded-video.md B6, D7, D13): every hour, in each
//! organization,
//!
//! 1. mark each incomplete upload whose row has not changed for 24 hours as terminating, to be
//!    deleted with its video (still `uploading`);
//! 2. mark the upload of each video that failed as `video.not_a_video` (B7) as terminating, to be
//!    deleted without its video; the chunk that completed it marks it at once, so this finds only
//!    one created before a mark existed. A video that failed for any other reason keeps its
//!    original for a retry (B2) and is left alone;
//! 3. finish every terminating upload ([`crate::video_parts::finish`]): its parts, then its row or
//!    its video. This also finishes a termination or a discard whose delete failed;
//! 4. fail as `video.transcode_failed` every `processing` video no live `video.transcode` job
//!    will finish: its job is dead, or it has none and has not changed for 24 hours
//!    (`stepclimb_core::transcodes::fail_stranded`). A job whose lease lapsed on its last
//!    attempt, or a process that could not transcode, leaves such a video; the author can then
//!    retry it (B2);
//! 5. delete the upload parts no upload row owns, last written [`ORPHAN_AGE`] ago or more
//!    ([`crate::video_parts::delete_orphaned_parts`], D45): a chunk's copy that landed after a
//!    termination had listed the parts leaves one, which no quota counts.
//!
//! Steps 1 and 2 mark each upload in a transaction that holds its row `FOR UPDATE`, the lock a
//! chunk takes ([`crate::video_uploads`]), and check again that it still applies; step 3 deletes
//! the parts holding no lock, since no chunk is stored for a marked upload. Without storage the
//! job does nothing: no upload can be started without it. A failure in one organization is logged
//! and the others still run; the job retries if any failed. Safe to run twice (spec m1/01 B12).

use std::time::{Duration, SystemTime};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope,
    id::Uuid,
    organizations, transcodes,
    videos::{self, Status, Terminating, Upload},
};
use stepclimb_jobs::{JobContext, Kind, Outcome, Registry, RegistryError};

use crate::{storage::Storage, video_parts};

/// The kind.
pub const CLEANUP: Kind = Kind::new("videos.cleanup", "default");

/// How often it runs.
pub const INTERVAL: Duration = Duration::from_secs(60 * 60);

/// How long an incomplete upload may go untouched before the job deletes it (B6).
pub const RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

/// How old an upload part no upload row owns must be before step 5 deletes it, by the job's
/// clock against the store's write time (D45). A copy still landing is seconds to minutes old.
pub const ORPHAN_AGE: Duration = Duration::from_secs(60 * 60);

/// Register `videos.cleanup` in `registry`, with the process's `storage`.
pub fn register(registry: &mut Registry, storage: Option<Storage>) -> Result<(), RegistryError> {
    registry.register_periodic(CLEANUP, INTERVAL, move |ctx: JobContext| {
        let storage = storage.clone();
        async move { cleanup(ctx, storage).await }
    })
}

async fn cleanup(ctx: JobContext, storage: Option<Storage>) -> Outcome {
    let Some(storage) = storage else {
        return Outcome::Done;
    };
    match run(ctx.pool(), &storage, ctx.clock().now()).await {
        Ok(report) if report.failed > 0 => Outcome::retry(format_args!(
            "could not clean up video uploads in {} places",
            report.failed
        )),
        Ok(_) => Outcome::Done,
        Err(error) => Outcome::retry(format_args!("could not list organizations: {error}")),
    }
}

/// What one run did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Stale uploads marked to be deleted with their videos (step 1).
    pub stale_marked: u64,
    /// Not-a-video uploads marked to have their parts deleted (step 2).
    pub not_a_video_marked: u64,
    /// Terminating uploads whose parts and rows were deleted (step 3).
    pub finished: u64,
    /// `processing` videos failed because no live job will finish them (step 4).
    pub stranded: u64,
    /// Upload parts no upload row owns, deleted (step 5).
    pub orphaned_parts: u64,
    /// Uploads or organizations that failed, each logged.
    pub failed: usize,
}

/// One run of the job at `now`, in every organization. Only failing to list the organizations
/// stops it.
pub async fn run(
    pool: &PgPool,
    storage: &Storage,
    now: SystemTime,
) -> Result<Report, organizations::Error> {
    let mut report = Report::default();
    let (Some(cutoff), Some(orphan_cutoff)) =
        (now.checked_sub(RETENTION), now.checked_sub(ORPHAN_AGE))
    else {
        return Ok(report);
    };
    for org_id in organizations::ids(pool).await? {
        let cleaner = Cleaner {
            pool,
            storage,
            scope: job_scope(org_id),
            cutoff,
            orphan_cutoff,
            now,
        };
        cleaner.run(&mut report).await;
    }
    let changed = report.stale_marked
        + report.not_a_video_marked
        + report.finished
        + report.stranded
        + report.orphaned_parts;
    if changed > 0 {
        tracing::info!(?report, "cleaned up video uploads");
    }
    Ok(report)
}

#[expect(
    clippy::disallowed_methods,
    reason = "spec 02 B9 allows the instance scope in a job; videos.cleanup acts in each organization in turn (spec m1/01 B10)"
)]
fn job_scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// Which uploads a marking step marks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Stale,
    NotAVideo,
}

/// One organization's run.
struct Cleaner<'a> {
    pool: &'a PgPool,
    storage: &'a Storage,
    scope: OrgScope,
    cutoff: SystemTime,
    orphan_cutoff: SystemTime,
    now: SystemTime,
}

impl Cleaner<'_> {
    async fn run(&self, report: &mut Report) {
        for step in [Step::Stale, Step::NotAVideo] {
            match self.mark_all(step, report).await {
                Ok(marked) if step == Step::Stale => report.stale_marked += marked,
                Ok(marked) => report.not_a_video_marked += marked,
                Err(error) => self.failed(report, &error, "could not list video uploads"),
            }
        }
        let terminating = match videos::terminating_uploads(self.pool, self.scope).await {
            Ok(uploads) => uploads,
            Err(error) => return self.failed(report, &error, "could not list terminating uploads"),
        };
        for upload in terminating {
            match video_parts::finish(self.pool, self.storage, self.scope, upload.id).await {
                Ok(done) => report.finished += u64::from(done),
                Err(error) => self.failed(report, &error, "could not delete a video upload"),
            }
        }
        match self.fail_stranded().await {
            Ok(failed) => report.stranded += failed,
            Err(error) => self.failed(report, &error, "could not fail stranded videos"),
        }
        let orphaned = video_parts::delete_orphaned_parts(
            self.pool,
            self.storage,
            self.scope,
            self.orphan_cutoff,
        );
        match orphaned.await {
            Ok(deleted) => report.orphaned_parts += u64::try_from(deleted).unwrap_or(u64::MAX),
            Err(error) => self.failed(report, &error, "could not delete orphaned upload parts"),
        }
    }

    /// Step 4 in its own transaction.
    async fn fail_stranded(&self) -> Result<u64, Error> {
        let mut tx = self.pool.begin().await?;
        let failed = transcodes::fail_stranded(&mut tx, self.scope, self.cutoff, self.now).await?;
        tx.commit().await?;
        for video_id in &failed {
            let org_id = self.scope.org_id();
            tracing::warn!(%org_id, %video_id, "failed a processing video no transcode job will finish");
        }
        Ok(u64::try_from(failed.len()).unwrap_or(u64::MAX))
    }

    fn failed(&self, report: &mut Report, error: &dyn std::error::Error, what: &str) {
        report.failed += 1;
        let org_id = self.scope.org_id();
        tracing::error!(%org_id, %error, "{what}");
    }

    /// Mark every upload `step` lists; how many were marked. A failed mark is counted and logged.
    async fn mark_all(&self, step: Step, report: &mut Report) -> Result<u64, videos::Error> {
        let uploads = match step {
            Step::Stale => videos::stale_uploads(self.pool, self.scope, self.cutoff).await?,
            Step::NotAVideo => videos::not_a_video_uploads(self.pool, self.scope).await?,
        };
        let mut marked = 0;
        for upload in uploads {
            match self.mark(step, upload.id).await {
                Ok(done) => marked += u64::from(done),
                Err(error) => self.failed(report, &error, "could not mark a video upload"),
            }
        }
        Ok(marked)
    }

    /// Under the upload row's lock, if `step` still applies, mark it terminating and commit.
    async fn mark(&self, step: Step, upload_id: Uuid) -> Result<bool, Error> {
        let mut tx = self.pool.begin().await?;
        let Some(upload) = videos::lock_upload(&mut tx, self.scope, upload_id).await? else {
            return Ok(false);
        };
        let video = videos::get(&mut tx, self.scope, upload.video_id).await?;
        if !self.applies(step, &upload, video.as_ref()) {
            return Ok(false);
        }
        let what = match step {
            Step::Stale => Terminating::Video,
            Step::NotAVideo => Terminating::Parts,
        };
        let marked =
            videos::mark_terminating(&mut tx, self.scope, upload.id, what, self.now).await?;
        tx.commit().await?;
        Ok(marked)
    }

    /// Whether `step` still applies to `upload`, read under its lock, whose video is `video`.
    fn applies(&self, step: Step, upload: &Upload, video: Option<&videos::Video>) -> bool {
        if upload.terminating.is_some() {
            return false;
        }
        let Some(video) = video else { return false };
        match step {
            Step::Stale => {
                video.status == Status::Uploading
                    && !upload.is_complete()
                    && upload.updated_at <= self.cutoff
            }
            Step::NotAVideo => {
                video.status == Status::Failed
                    && video.error_code.as_deref() == Some(videos::NOT_A_VIDEO)
            }
        }
    }
}

/// Why one upload could not be marked.
#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Videos(#[from] videos::Error),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}
