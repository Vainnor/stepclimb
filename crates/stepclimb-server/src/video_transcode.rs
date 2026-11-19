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
//!    `video.not_a_video` (its parts are deleted, as B7's do), an unreadable file or an
//!    oversized frame `video.probe_failed`, a duration over `max_video_seconds`
//!    `video.too_long` (B9).
//! 4. The probed minutes are checked against `transcode_minutes` and charged in one transaction
//!    under the plan row's lock, so concurrent transcodes cannot overshoot the limit; minutes
//!    that do not fit fail the video `entitlement.limit_reached` (B9, B13, owner decision
//!    2026-11-17).
//! 5. ffmpeg renders the outputs under B10's limits, every output cut at the probed duration; a
//!    run that fails or breaks a limit fails the video `video.transcode_failed`, and only the
//!    tail of ffmpeg's stderr reaches the server's log.
//! 6. The outputs go to `v<n>/`; one transaction sets the video `ready` if it is still at run
//!    `n`, marks the upload's parts for deletion and re-checks `video_storage` with the real
//!    size, which fails the video and deletes the outputs if it does not fit (B12).
//! 7. After the commit the parts and the upload row are deleted (Q3), and so are the outputs of
//!    every other run, which a player of the old run could still be reading until then (T7).
//!
//! The job holds its lease through steps 2 to 6, retrying a failed extension until little of
//! the lease is left. Every way a run ends goes through `Job::end`: a failure of the database,
//! the store or the host, a lost lease, a panic, or a process that cannot transcode is the
//! server's side, which refunds the charge recorded on the video row and is retried with the
//! kind's backoff; on
//! the last attempt it fails the video `video.transcode_failed`, which an author can retry
//! (B2). A failure the input causes keeps the charge. A job whose lease lapses on its last
//! attempt never reaches its handler; `videos.cleanup` fails its video
//! ([`transcodes::fail_stranded`]). The work directory is removed whatever happens, and no
//! error text reaches a learner: the video carries only its error code.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use object_store::path::Path;
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope,
    feature::Limit,
    id::Uuid,
    org_plans::{self, Room},
    sniff,
    transcodes::{self, Fault, Ready, Start},
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

/// How long a failed lease extension waits before it tries again.
pub const LEASE_RETRY: Duration = Duration::from_secs(15);

/// How much of the lease must be left for another extension attempt: below it the job stops
/// rather than risk running on after another worker has taken it.
pub const LEASE_MARGIN: Duration = Duration::from_secs(60);

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
    let taken = Arc::new(Mutex::new(None));
    let owned = (
        ctx.clone(),
        storage.clone(),
        transcoder.clone(),
        Arc::clone(&taken),
    );
    let run = async move {
        let (ctx, storage, transcoder, taken) = owned;
        let job = Job {
            ctx: &ctx,
            storage: storage.as_ref(),
            transcoder: transcoder.as_ref(),
            scope,
            video_id,
            taken: &taken,
        };
        job.run().await
    };
    guarded(run, |panic| async move {
        tracing::error!(%video_id, "a transcode task {panic}");
        let job = Job {
            ctx: &ctx,
            storage: storage.as_ref(),
            transcoder: transcoder.as_ref(),
            scope,
            video_id,
            taken: &taken,
        };
        let run = *taken.lock().unwrap_or_else(PoisonError::into_inner);
        job.end(run, Stop::Transient(format!("the transcode task {panic}")))
            .await
    })
    .await
}

/// Run `task` on a task of its own; if it panics or is cancelled, the outcome `on_abort` gives
/// for a description of how it ended, so a panic is settled like any failure on the server's
/// side instead of leaving the job to its lease.
async fn guarded<F, A, Fut>(task: F, on_abort: A) -> Outcome
where
    F: Future<Output = Outcome> + Send + 'static,
    A: FnOnce(&'static str) -> Fut,
    Fut: Future<Output = Outcome>,
{
    match tokio::spawn(task).await {
        Ok(outcome) => outcome,
        Err(error) if error.is_panic() => on_abort("panicked").await,
        Err(_) => on_abort("was cancelled").await,
    }
}

/// One claimed transcode. `storage` and `transcoder` are `None` in a process that cannot
/// transcode, whose job only settles.
struct Job<'a> {
    ctx: &'a JobContext,
    storage: Option<&'a Storage>,
    transcoder: Option<&'a Transcoder>,
    scope: OrgScope,
    video_id: Uuid,
    /// The run this job took, once [`transcodes::start`] gave it one; shared with [`handle`],
    /// which settles a panic at that run.
    taken: &'a Mutex<Option<i32>>,
}

impl Job<'_> {
    fn pool(&self) -> &PgPool {
        self.ctx.pool()
    }

    fn now(&self) -> std::time::SystemTime {
        self.ctx.clock().now()
    }

    async fn run(&self) -> Outcome {
        let (Some(storage), Some(transcoder)) = (self.storage, self.transcoder) else {
            let stop =
                Stop::Transient("this process cannot transcode: no storage or no ffmpeg".into());
            return self.end(None, stop).await;
        };
        let (run, upload) = match self.start().await {
            Ok(Start::Run { run, upload }) => (run, upload),
            Ok(Start::NotProcessing) => return Outcome::Done,
            Ok(Start::NoOriginal { run }) => {
                return self
                    .end(Some(run), Stop::Video(transcodes::TRANSCODE_FAILED))
                    .await;
            }
            Err(stop) => return self.end(None, stop).await,
        };
        *self.taken.lock().unwrap_or_else(PoisonError::into_inner) = Some(run);
        let transcoded = tokio::select! {
            transcoded = self.transcode(storage, transcoder, run, &upload) => transcoded,
            stop = self.hold_lease() => Err(stop),
        };
        match transcoded {
            Ok(()) => {
                // After the commit, outside the lease's race, so a lost lease cannot cut it off.
                self.clean_up(storage, run, &upload).await;
                Outcome::Done
            }
            Err(stop) => self.end(Some(run), stop).await,
        }
    }

    /// Step 1 in its own transaction.
    async fn start(&self) -> Result<Start, Stop> {
        let mut tx = self.pool().begin().await?;
        let started = transcodes::start(&mut tx, self.scope, self.video_id, self.now()).await?;
        tx.commit().await?;
        Ok(started)
    }

    /// Settle a run that stopped short. A stop on the server's side (transient, superseded)
    /// refunds the run's charge recorded on the video row, then is retried or, on the last
    /// attempt, fails the video as [`Fault::Server`]; a stop the input caused fails the video as
    /// [`Fault::Input`] and keeps the charge. `run` is `None` when the job stopped before it took
    /// a run.
    async fn end(&self, run: Option<i32>, stop: Stop) -> Outcome {
        let job = self.ctx.job();
        let (code, fault, outcome) = match stop {
            Stop::Superseded => {
                if let Some(run) = run {
                    self.refund(run).await;
                }
                return Outcome::Done;
            }
            Stop::Video(code) => (code, Fault::Input, Outcome::Done),
            Stop::Transient(error) if job.attempt < job.max_attempts => {
                tracing::warn!(video_id = %self.video_id, %error, "a transcode will be retried");
                // Without a run, the charge on the row may be a newer run's: the next start
                // refunds a dead run's instead.
                if let Some(run) = run {
                    self.refund(run).await;
                }
                return Outcome::Retry(error);
            }
            Stop::Transient(error) => (
                transcodes::TRANSCODE_FAILED,
                Fault::Server,
                Outcome::Fail(error),
            ),
        };
        match self.fail(run, (code, fault)).await {
            Ok(()) => outcome,
            Err(error) => Outcome::retry(format_args!("could not fail the video: {error:?}")),
        }
    }

    /// Give back the charge recorded for run `run`, if any. A failure is logged: the record stays,
    /// and the next run's start or `videos.cleanup` refunds it.
    async fn refund(&self, run: i32) {
        let refunded = async {
            let mut tx = self.pool().begin().await?;
            transcodes::refund_open(&mut tx, self.scope, self.video_id, Some(run)).await?;
            tx.commit().await?;
            Ok::<(), Stop>(())
        };
        if let Err(error) = refunded.await {
            tracing::error!(video_id = %self.video_id, ?error, "could not refund a failed transcode's minutes");
        }
    }

    /// Fail the video with `code` at run `run` (at any run when `None`), as `fault` caused it.
    /// Only once the row has changed are the run's outputs deleted, so a video that is no longer
    /// at this run (a newer run made it ready) keeps them; a video that is not a video has its
    /// upload's parts deleted too (D7).
    async fn fail(
        &self,
        run: Option<i32>,
        (code, fault): (&'static str, Fault),
    ) -> Result<(), Stop> {
        tracing::info!(video_id = %self.video_id, code, ?fault, "a transcode failed the video");
        let mut tx = self.pool().begin().await?;
        let now = self.now();
        let (scope, video) = (self.scope, self.video_id);
        let failed = match run {
            Some(run) => transcodes::fail(&mut tx, scope, (video, run), (code, fault), now).await?,
            None => transcodes::fail_any_run(&mut tx, scope, video, (code, fault), now).await?,
        };
        let mut discard = None;
        if failed && code == videos::NOT_A_VIDEO {
            let upload = videos::lock_upload_of_video(&mut tx, scope, video).await?;
            if let Some(upload) = upload {
                videos::mark_terminating(&mut tx, scope, upload.id, Terminating::Parts, now)
                    .await?;
                discard = Some(upload.id);
            }
        }
        tx.commit().await?;
        let Some(storage) = self.storage else {
            return Ok(());
        };
        if let (true, Some(run)) = (failed, run) {
            video_parts::delete_tree(storage, &self.run_prefix(storage, run)).await?;
        }
        if let Some(upload_id) = discard
            && let Err(error) = video_parts::finish(self.pool(), storage, scope, upload_id).await
        {
            tracing::warn!(%error, %upload_id, "could not delete the parts of a video that is not a video; videos.cleanup will");
        }
        Ok(())
    }

    /// `orgs/<org>/videos/<id>/v<run>/`.
    fn run_prefix(&self, storage: &Storage, run: i32) -> Path {
        let video = storage.video_prefix(self.scope, self.video_id);
        Path::from_iter(
            video
                .parts()
                .chain(std::iter::once(format!("v{run}").as_str().into())),
        )
    }

    /// Steps 2 to 7 for run `run` of `upload`.
    async fn transcode(
        &self,
        storage: &Storage,
        transcoder: &Transcoder,
        run: i32,
        upload: &Upload,
    ) -> Result<(), Stop> {
        let work = WorkDir::create(&transcoder.dir)?;
        let input = work.path().join("input");
        match video_files::concatenate(storage, self.scope, upload, &input).await {
            // `start` found the upload complete, so missing parts are the store's fault.
            Err(video_files::Error::Incomplete) => {
                return Err(Stop::Transient(
                    "the stored parts no longer make up the upload".into(),
                ));
            }
            other => other?,
        }
        let head = video_files::head(&input, sniff::HEAD_BYTES).await?;
        let container = sniff::video(&head)
            .and_then(Container::from_sniffed)
            .ok_or(Stop::Video(videos::NOT_A_VIDEO))?;
        let plan = self
            .probe(transcoder, container, &input, work.path())
            .await?;
        self.charge(run, &plan).await?;
        let out = work.path().join("out");
        std::fs::create_dir(&out)?;
        self.render(transcoder, &plan, &input, &out).await?;
        let prefix = self.run_prefix(storage, run);
        let bytes = match video_files::upload_outputs(storage, &out, &prefix).await {
            Err(video_files::Error::NotAFile(name)) => {
                tracing::warn!(video_id = %self.video_id, %name, "ffmpeg left something other than a file among the outputs");
                return Err(Stop::Transient(format!(
                    "the output {name} is not a regular file"
                )));
            }
            other => other?,
        };
        self.finish(storage, (run, upload), &plan, bytes).await?;
        Ok(())
    }

    /// B9: probe `input`, and refuse a file with no video, an unreadable one and one too long.
    async fn probe(
        &self,
        transcoder: &Transcoder,
        container: Container,
        input: &std::path::Path,
        work: &std::path::Path,
    ) -> Result<Plan, Stop> {
        let ffprobe = &transcoder.tools.ffprobe;
        let source = match probe::probe(ffprobe, container, input, work).await {
            Ok(Probed::Video(source)) => source,
            Ok(Probed::NoVideo) => return Err(Stop::Video(videos::NOT_A_VIDEO)),
            Err(ProbeError::Unreadable) => return Err(Stop::Video(transcodes::PROBE_FAILED)),
            Err(ProbeError::Run(error)) if error.is_input_fault() => {
                let stderr = stderr_of(&error);
                tracing::info!(video_id = %self.video_id, %error, stderr = ?stderr, "ffprobe could not read a video");
                return Err(Stop::Video(transcodes::PROBE_FAILED));
            }
            Err(ProbeError::Run(error)) => return Err(error.into()),
        };
        if source.duration_s > f64::from(transcoder.max_video_seconds) {
            return Err(Stop::Video(transcodes::TOO_LONG));
        }
        Ok(Plan::new(container, source))
    }

    /// B9, B13, Q4: check the video's minutes against `transcode_minutes` and charge them, in
    /// one transaction under the plan row's lock, so two transcodes of one organization take
    /// turns and cannot both pass at the limit (owner decision, 2026-11-17). The charge is
    /// recorded on the video row in the same transaction, so a run that never ends is refunded
    /// by the next run's start.
    async fn charge(&self, run: i32, plan: &Plan) -> Result<(), Stop> {
        let (minutes, now) = (transcodes::minutes(plan.source.duration_s), self.now());
        let mut tx = self.pool().begin().await?;
        if let Room::Full { .. } = self.room(&mut tx, Limit::TranscodeMinutes, minutes).await? {
            tx.rollback().await?;
            return Err(Stop::Video(transcodes::LIMIT_REACHED));
        }
        let charged =
            transcodes::charge(&mut tx, self.scope, (self.video_id, run), minutes, now).await?;
        tx.commit().await?;
        if charged {
            Ok(())
        } else {
            Err(Stop::Superseded)
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

    /// B10, B11: run ffmpeg.
    async fn render(
        &self,
        transcoder: &Transcoder,
        plan: &Plan,
        input: &std::path::Path,
        out: &std::path::Path,
    ) -> Result<(), Stop> {
        let render = Render {
            ffmpeg: &transcoder.tools.ffmpeg,
            threads: transcoder.threads,
            input,
            out,
        };
        match plan::render(plan, render).await {
            Ok(()) => Ok(()),
            Err(error) if error.is_input_fault() => {
                let stderr = stderr_of(&error);
                tracing::warn!(video_id = %self.video_id, %error, stderr = ?stderr, "ffmpeg could not transcode a video");
                Err(Stop::Video(transcodes::TRANSCODE_FAILED))
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Extend the lease every [`LEASE_EVERY`]. A failed extension is tried again every
    /// [`LEASE_RETRY`] while [`keep_trying`] allows; ends only when the lease is lost or no
    /// extension succeeded in time.
    async fn hold_lease(&self) -> Stop {
        let mut held_at = tokio::time::Instant::now();
        loop {
            tokio::time::sleep(LEASE_EVERY).await;
            loop {
                match self.ctx.extend_lease().await {
                    Ok(Lease::Held) => {
                        held_at = tokio::time::Instant::now();
                        break;
                    }
                    Ok(Lease::Lost) => return Stop::Superseded,
                    Err(error) if keep_trying(held_at.elapsed()) => {
                        tracing::warn!(video_id = %self.video_id, %error, "could not extend a transcode's lease; trying again");
                        tokio::time::sleep(LEASE_RETRY).await;
                    }
                    Err(error) => return Stop::Transient(error.to_string()),
                }
            }
        }
    }

    /// B12: in one transaction, `ready` at run `run`, the upload's parts marked for deletion and
    /// `video_storage` re-checked with the outputs' `bytes`. The minutes were charged at step 4.
    async fn finish(
        &self,
        storage: &Storage,
        (run, upload): (i32, &Upload),
        plan: &Plan,
        bytes: u64,
    ) -> Result<(), Stop> {
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
            video_parts::delete_tree(storage, &self.run_prefix(storage, run)).await?;
            return Err(Stop::Superseded);
        }
        videos::mark_terminating(&mut tx, self.scope, upload.id, Terminating::Parts, now).await?;
        if let Room::Full { .. } = self.room(&mut tx, Limit::VideoStorage, 0).await? {
            drop(tx);
            return Err(Stop::Video(transcodes::LIMIT_REACHED));
        }
        tx.commit().await?;
        Ok(())
    }

    /// Step 6: the upload's parts and row, and every other run's outputs. A failure is logged;
    /// `videos.cleanup` finishes the parts.
    async fn clean_up(&self, storage: &Storage, run: i32, upload: &Upload) {
        if let Err(error) = video_parts::finish(self.pool(), storage, self.scope, upload.id).await {
            tracing::warn!(%error, upload_id = %upload.id, "could not delete a transcoded upload's parts; videos.cleanup will");
        }
        if let Err(error) =
            video_parts::delete_other_runs(storage, self.scope, self.video_id, run).await
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

/// Whether a failed lease extension may be tried again, `since_held` after the last one that
/// succeeded: only while another try and [`LEASE_MARGIN`] still fit in the lease.
fn keep_trying(since_held: Duration) -> bool {
    since_held + LEASE_RETRY + LEASE_MARGIN < LEASE
}

/// The stderr tail of a failed run, for the log.
fn stderr_of(error: &stepclimb_media::run::RunError) -> &str {
    match error {
        stepclimb_media::run::RunError::Failed { stderr, .. } => stderr,
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_extension_is_retried_while_the_lease_has_room() {
        assert!(keep_trying(LEASE_EVERY));
        assert!(keep_trying(Duration::from_secs(8 * 60 + 44)));
        assert!(!keep_trying(Duration::from_secs(8 * 60 + 45)));
        assert!(!keep_trying(LEASE));
    }

    #[tokio::test]
    async fn a_panicking_transcode_is_settled_by_the_abort_outcome() {
        let panicking = async {
            if Outcome::Done == Outcome::Done {
                panic!("a transcode bug");
            }
            Outcome::Done
        };
        let outcome = guarded(panicking, |how| async move { Outcome::retry(how) }).await;
        assert_eq!(outcome, Outcome::Retry("panicked".to_owned()));
        let fine = guarded(async { Outcome::Done }, |_| async { Outcome::fail("no") }).await;
        assert_eq!(fine, Outcome::Done);
    }
}
