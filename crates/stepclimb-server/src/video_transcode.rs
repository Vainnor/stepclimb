// SPDX-License-Identifier: AGPL-3.0-or-later
//! `video.transcode` on the `transcode` queue (docs/specs/m1/08-uploaded-video.md B8 to B13).
//!
//! One job turns one `processing` video into a `ready` one:
//!
//! 1. [`transcodes::start`] takes the next output run `n` under the video's lock, or finds the
//!    video no longer `processing` and does nothing (B12: a second run of a finished job).
//! 2. The upload's parts are concatenated into `input` in a fresh work directory under
//!    `transcode_dir` ([`video_files::concatenate`]); the file's first bytes pick the demuxer.
//! 3. ffprobe reads it with that demuxer pinned: no video stream fails the video
//!    `video.not_a_video` (its parts are deleted, as B7's do), an unreadable file
//!    `video.probe_failed`, a duration over `max_video_seconds` `video.too_long`, and minutes
//!    that do not fit `transcode_minutes` `entitlement.limit_reached` (B9).
//! 4. ffmpeg renders the outputs under B10's limits while the job extends its lease; a run that
//!    fails or breaks a limit fails the video `video.transcode_failed`, and only the tail of
//!    ffmpeg's stderr reaches the server's log.
//! 5. The outputs go to `v<n>/`; one transaction sets the video `ready` if it is still at run
//!    `n`, marks the upload's parts for deletion, records the minutes (B13) and re-checks
//!    `video_storage` with the real size, which fails the video and deletes the outputs if it
//!    does not fit (B12).
//! 6. After the commit the parts and the upload row are deleted (Q3), and so are the outputs of
//!    every other run, which a player of the old run could still be reading until then (T7).
//!
//! A failure of the database, the store or the host is retried with the kind's backoff; on the
//! last attempt it fails the video `video.transcode_failed`, which an author can retry (B2). The
//! work directory is removed whatever happens, and no error text reaches a learner: the video
//! carries only its error code.

use std::path::PathBuf;
use std::time::Duration;

use object_store::path::Path;
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope,
    feature::Limit,
    id::Uuid,
    org_plans::{self, Room},
    sniff,
    transcodes::{self, Ready, Start},
    videos::{self, Terminating, Upload},
};
use stepclimb_jobs::{JobContext, Kind, Lease, Outcome, Registry, RegistryError, Retry};
use stepclimb_media::{
    plan::{self, Plan, Render},
    probe::{self, Container, ProbeError, Probed},
    tools::Tools,
    workdir::WorkDir,
};

use crate::{storage::Storage, video_files, video_parts};

/// How long a claim holds a transcode before another worker may take it; the job extends it
/// every [`LEASE_EVERY`] while it runs.
pub const LEASE: Duration = Duration::from_secs(10 * 60);

/// How often a running transcode extends its lease.
pub const LEASE_EVERY: Duration = Duration::from_secs(2 * 60);

/// The job that transcodes a video once its upload is complete (B7, B9): 5 attempts, from 1
/// minute apart up to 30.
pub const TRANSCODE: Kind = Kind::new("video.transcode", "transcode")
    .with_lease(LEASE)
    .with_retry(Retry {
        base: Duration::from_secs(60),
        cap: Duration::from_secs(30 * 60),
        max_attempts: 5,
    });

/// What a process that serves the `transcode` queue runs ffmpeg with.
#[derive(Clone, Debug)]
pub struct Transcoder {
    pub tools: Tools,
    /// `transcode_threads`.
    pub threads: u32,
    /// `transcode_dir`.
    pub dir: PathBuf,
    /// `max_video_seconds`.
    pub max_video_seconds: u32,
}

/// Register `video.transcode` in `registry`, with the process's `storage` and `transcoder`
/// (`None` in a process that does not serve the queue, whose worker never claims the job).
pub fn register(
    registry: &mut Registry,
    storage: Option<Storage>,
    transcoder: Option<Transcoder>,
) -> Result<(), RegistryError> {
    registry.register(TRANSCODE, move |ctx: JobContext| {
        let (storage, transcoder) = (storage.clone(), transcoder.clone());
        async move { handle(ctx, storage, transcoder).await }
    })
}

/// How a transcode ended short of `ready`.
#[derive(Debug)]
enum Stop {
    /// The video fails with this code; the job is done.
    Video(&'static str),
    /// Something outside the video failed; the job is retried.
    Transient(String),
    /// Another worker took the job, or the video moved on; nothing more to do.
    Superseded,
}

impl<E: std::error::Error> From<E> for Stop {
    fn from(error: E) -> Self {
        Self::Transient(error.to_string())
    }
}

async fn handle(
    ctx: JobContext,
    storage: Option<Storage>,
    transcoder: Option<Transcoder>,
) -> Outcome {
    let Some(scope) = ctx.scope() else {
        return Outcome::fail("video.transcode is an organization job");
    };
    let Some(video_id) = ctx.payload()["video_id"]
        .as_str()
        .and_then(|v| v.parse().ok())
    else {
        return Outcome::fail("the payload names no video_id");
    };
    let (Some(storage), Some(transcoder)) = (storage, transcoder) else {
        return Outcome::retry("this process cannot transcode: no storage or no ffmpeg");
    };
    let job = Job {
        ctx: &ctx,
        storage: &storage,
        transcoder: &transcoder,
        scope,
        video_id,
    };
    job.run().await
}

/// One claimed transcode.
struct Job<'a> {
    ctx: &'a JobContext,
    storage: &'a Storage,
    transcoder: &'a Transcoder,
    scope: OrgScope,
    video_id: Uuid,
}

impl Job<'_> {
    fn pool(&self) -> &PgPool {
        self.ctx.pool()
    }

    fn now(&self) -> std::time::SystemTime {
        self.ctx.clock().now()
    }

    async fn run(&self) -> Outcome {
        let mut tx = match self.pool().begin().await {
            Ok(tx) => tx,
            Err(error) => return Outcome::retry(error),
        };
        let started = transcodes::start(&mut tx, self.scope, self.video_id, self.now()).await;
        let (run, upload) = match started {
            Ok(Start::Run { run, upload }) => (run, upload),
            Ok(Start::NotProcessing) => return Outcome::Done,
            Ok(Start::NoOriginal { run }) => {
                drop(tx);
                return self
                    .end(run, Stop::Video(transcodes::TRANSCODE_FAILED))
                    .await;
            }
            Err(error) => return Outcome::retry(error),
        };
        if let Err(error) = tx.commit().await {
            return Outcome::retry(error);
        }
        match self.transcode(run, &upload).await {
            Ok(()) => Outcome::Done,
            Err(stop) => self.end(run, stop).await,
        }
    }

    /// Settle a run that stopped short: fail the video when the stop says so, or when a
    /// transient failure used the last attempt.
    async fn end(&self, run: i32, stop: Stop) -> Outcome {
        let job = self.ctx.job();
        let (code, outcome) = match stop {
            Stop::Superseded => return Outcome::Done,
            Stop::Video(code) => (code, Outcome::Done),
            Stop::Transient(error) if job.attempt < job.max_attempts => {
                tracing::warn!(video_id = %self.video_id, %error, "a transcode will be retried");
                return Outcome::Retry(error);
            }
            Stop::Transient(error) => (transcodes::TRANSCODE_FAILED, Outcome::Fail(error)),
        };
        match self.fail(run, code).await {
            Ok(()) => outcome,
            Err(error) => Outcome::retry(format_args!("could not fail the video: {error:?}")),
        }
    }

    /// Fail the video at run `run` with `code`, deleting the run's outputs first; a video that
    /// is not a video has its upload's parts deleted too (D7).
    async fn fail(&self, run: i32, code: &'static str) -> Result<(), Stop> {
        tracing::info!(video_id = %self.video_id, code, "a transcode failed the video");
        video_parts::delete_tree(self.storage, &self.run_prefix(run)).await?;
        let mut tx = self.pool().begin().await?;
        let now = self.now();
        let failed = transcodes::fail(&mut tx, self.scope, (self.video_id, run), code, now).await?;
        let mut discard = None;
        if failed && code == videos::NOT_A_VIDEO {
            let upload = videos::lock_upload_of_video(&mut tx, self.scope, self.video_id).await?;
            if let Some(upload) = upload {
                videos::mark_terminating(&mut tx, self.scope, upload.id, Terminating::Parts, now)
                    .await?;
                discard = Some(upload.id);
            }
        }
        tx.commit().await?;
        if let Some(upload_id) = discard
            && let Err(error) =
                video_parts::finish(self.pool(), self.storage, self.scope, upload_id).await
        {
            tracing::warn!(%error, %upload_id, "could not delete the parts of a video that is not a video; videos.cleanup will");
        }
        Ok(())
    }

    /// `orgs/<org>/videos/<id>/v<run>/`.
    fn run_prefix(&self, run: i32) -> Path {
        let video = self.storage.video_prefix(self.scope, self.video_id);
        Path::from_iter(
            video
                .parts()
                .chain(std::iter::once(format!("v{run}").as_str().into())),
        )
    }

    /// Steps 2 to 6 for run `run` of `upload`.
    async fn transcode(&self, run: i32, upload: &Upload) -> Result<(), Stop> {
        let work = WorkDir::create(&self.transcoder.dir)?;
        let input = work.path().join("input");
        match video_files::concatenate(self.storage, self.scope, upload, &input).await {
            Err(video_files::Error::Incomplete) => {
                return Err(Stop::Video(transcodes::TRANSCODE_FAILED));
            }
            other => other?,
        }
        let head = video_files::head(&input, sniff::HEAD_BYTES).await?;
        let container = sniff::video(&head)
            .and_then(Container::from_sniffed)
            .ok_or(Stop::Video(videos::NOT_A_VIDEO))?;
        let plan = self.probe(container, &input, work.path()).await?;
        self.check_minutes(&plan).await?;
        let out = work.path().join("out");
        std::fs::create_dir(&out)?;
        self.render(&plan, &input, &out).await?;
        let prefix = self.run_prefix(run);
        let bytes = video_files::upload_outputs(self.storage, &out, &prefix).await?;
        self.finish(run, upload, &plan, bytes).await?;
        self.clean_up(run, upload).await;
        Ok(())
    }

    /// B9: probe `input`, and refuse a file with no video, an unreadable one and one too long.
    async fn probe(
        &self,
        container: Container,
        input: &std::path::Path,
        work: &std::path::Path,
    ) -> Result<Plan, Stop> {
        let ffprobe = &self.transcoder.tools.ffprobe;
        let source = match probe::probe(ffprobe, container, input, work).await {
            Ok(Probed::Video(source)) => source,
            Ok(Probed::NoVideo) => return Err(Stop::Video(videos::NOT_A_VIDEO)),
            Err(ProbeError::Unreadable) => return Err(Stop::Video(transcodes::PROBE_FAILED)),
            Err(ProbeError::Run(error)) if error.is_input_fault() => {
                let stderr = stderr_of(&error);
                tracing::info!(video_id = %self.video_id, %error, stderr, "ffprobe could not read a video");
                return Err(Stop::Video(transcodes::PROBE_FAILED));
            }
            Err(ProbeError::Run(error)) => return Err(error.into()),
        };
        if source.duration_s > f64::from(self.transcoder.max_video_seconds) {
            return Err(Stop::Video(transcodes::TOO_LONG));
        }
        Ok(Plan::new(container, source))
    }

    /// B9, Q4: the video's minutes must fit `transcode_minutes` this month. Only checked here;
    /// they are recorded when the transcode succeeds (B13).
    async fn check_minutes(&self, plan: &Plan) -> Result<(), Stop> {
        let mut tx = self.pool().begin().await?;
        let room = self
            .room(
                &mut tx,
                Limit::TranscodeMinutes,
                transcodes::minutes(plan.source.duration_s),
            )
            .await?;
        tx.rollback().await?;
        match room {
            Room::Fits => Ok(()),
            Room::Full { .. } => Err(Stop::Video(transcodes::LIMIT_REACHED)),
        }
    }

    async fn room(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        limit: Limit,
        adding: u64,
    ) -> Result<Room, Stop> {
        let plan = org_plans::get_with_plan(&mut **tx, self.scope).await?;
        let room = org_plans::check_room(
            tx,
            self.scope,
            &plan.entitlements,
            limit,
            adding,
            self.now(),
        )
        .await?;
        Ok(room)
    }

    /// B10, B11: run ffmpeg, extending the lease meanwhile.
    async fn render(
        &self,
        plan: &Plan,
        input: &std::path::Path,
        out: &std::path::Path,
    ) -> Result<(), Stop> {
        let render = Render {
            ffmpeg: &self.transcoder.tools.ffmpeg,
            threads: self.transcoder.threads,
            input,
            out,
        };
        let rendered = tokio::select! {
            rendered = plan::render(plan, render) => rendered,
            lost = self.hold_lease() => return Err(lost),
        };
        match rendered {
            Ok(()) => Ok(()),
            Err(error) if error.is_input_fault() => {
                let stderr = stderr_of(&error);
                tracing::warn!(video_id = %self.video_id, %error, stderr, "ffmpeg could not transcode a video");
                Err(Stop::Video(transcodes::TRANSCODE_FAILED))
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Extend the lease every [`LEASE_EVERY`]; ends only when the lease is lost or cannot be
    /// extended.
    async fn hold_lease(&self) -> Stop {
        loop {
            tokio::time::sleep(LEASE_EVERY).await;
            match self.ctx.extend_lease().await {
                Ok(Lease::Held) => {}
                Ok(Lease::Lost) => return Stop::Superseded,
                Err(error) => return Stop::Transient(error.to_string()),
            }
        }
    }

    /// B12, B13: in one transaction, `ready` at run `run`, the upload's parts marked for
    /// deletion, the minutes recorded and `video_storage` re-checked with the outputs' `bytes`.
    async fn finish(&self, run: i32, upload: &Upload, plan: &Plan, bytes: u64) -> Result<(), Stop> {
        let renditions = self.renditions(run, plan);
        let poster = self.key(run, plan::POSTER);
        let ready = Ready {
            duration_s: plan.source.duration_s,
            renditions: &renditions,
            poster_key: &poster,
            storage_bytes: bytes,
        };
        let now = self.now();
        let mut tx = self.pool().begin().await?;
        if !transcodes::finish(&mut tx, self.scope, (self.video_id, run), &ready, now).await? {
            drop(tx);
            video_parts::delete_tree(self.storage, &self.run_prefix(run)).await?;
            return Err(Stop::Superseded);
        }
        videos::mark_terminating(&mut tx, self.scope, upload.id, Terminating::Parts, now).await?;
        let minutes = transcodes::minutes(plan.source.duration_s);
        transcodes::record_minutes(&mut tx, self.scope, minutes, now).await?;
        if let Room::Full { .. } = self.room(&mut tx, Limit::VideoStorage, 0).await? {
            drop(tx);
            return Err(Stop::Video(transcodes::LIMIT_REACHED));
        }
        tx.commit().await?;
        Ok(())
    }

    /// Step 6: the upload's parts and row, and every other run's outputs. A failure is logged;
    /// `videos.cleanup` finishes the parts.
    async fn clean_up(&self, run: i32, upload: &Upload) {
        if let Err(error) =
            video_parts::finish(self.pool(), self.storage, self.scope, upload.id).await
        {
            tracing::warn!(%error, upload_id = %upload.id, "could not delete a transcoded upload's parts; videos.cleanup will");
        }
        if let Err(error) =
            video_parts::delete_other_runs(self.storage, self.scope, self.video_id, run).await
        {
            tracing::warn!(%error, video_id = %self.video_id, "could not delete the outputs of earlier transcode runs");
        }
    }

    /// The store key of `name` in run `run`.
    fn key(&self, run: i32, name: &str) -> String {
        format!(
            "orgs/{}/videos/{}/v{run}/{name}",
            self.scope.org_id(),
            self.video_id
        )
    }

    /// B12's `renditions`: heights, bitrates, and the keys of the playlists, the MP4 and the
    /// sprite index.
    fn renditions(&self, run: i32, plan: &Plan) -> serde_json::Value {
        let playlists: Vec<String> = plan
            .renditions
            .iter()
            .map(|r| {
                self.key(
                    run,
                    &format!("{}/{}", plan::rendition_dir(r.height), plan::MEDIA_PLAYLIST),
                )
            })
            .collect();
        serde_json::json!({
            "run": run,
            "heights": plan.renditions.iter().map(|r| r.height).collect::<Vec<_>>(),
            "bitrates": plan.renditions.iter().map(|r| r.video_bitrate).collect::<Vec<_>>(),
            "master": self.key(run, plan::MASTER_PLAYLIST),
            "playlists": playlists,
            "mp4": self.key(run, plan::MP4),
            "mp4_height": plan.mp4.height,
            "sprites": self.key(run, plan::SPRITE_INDEX),
        })
    }
}

/// The stderr tail of a failed run, for the log.
fn stderr_of(error: &stepclimb_media::run::RunError) -> &str {
    match error {
        stepclimb_media::run::RunError::Failed { stderr, .. } => stderr,
        _ => "",
    }
}
