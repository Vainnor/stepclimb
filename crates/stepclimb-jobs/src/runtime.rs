// SPDX-License-Identifier: AGPL-3.0-or-later
//! The worker loop a server runs (docs/specs/m1/01-job-queue-and-outbox.md B14, B15, Q2).
//!
//! [`Worker::run`] keeps up to `concurrency` jobs running. Whenever it has room it claims as
//! many due jobs as fit; when none are due it waits for the poll interval, or for a running job
//! to end, before it looks again. When `shutdown` completes it claims nothing more, waits up to
//! the grace period for the jobs it is running, and returns; a job still running then keeps its
//! lease, which runs out so another worker takes the job (B5).

use std::future::Future;
use std::num::NonZeroU32;
use std::pin::pin;
use std::time::Duration;

use tokio::task::JoinSet;

use crate::worker::{Worker, report_ended};

/// How often an idle worker looks for due jobs (Q2).
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// How long a stopping worker waits for its running jobs (B15).
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(30);

/// How a worker loop runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunOptions {
    /// The most jobs it runs at once (`worker_concurrency`, B14).
    pub concurrency: NonZeroU32,
    /// How long it waits between looks when no job is due.
    pub poll: Duration,
    /// How long it waits for running jobs once told to stop.
    pub grace: Duration,
}

impl RunOptions {
    /// `concurrency` jobs at once, with [`POLL_INTERVAL`] and [`SHUTDOWN_GRACE`].
    #[must_use]
    pub fn new(concurrency: NonZeroU32) -> Self {
        Self {
            concurrency,
            poll: POLL_INTERVAL,
            grace: SHUTDOWN_GRACE,
        }
    }
}

/// How a worker loop ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stopped {
    /// Every job it was running ended within the grace period.
    Drained,
    /// This many jobs were still running when the grace period ran out; their leases run out
    /// and other workers take them.
    GraceExpired { running: usize },
}

impl Worker {
    /// Claim and run due jobs until `shutdown` completes, then stop as [`RunOptions::grace`]
    /// allows. A failed claim is logged and tried again after the poll interval.
    pub async fn run(&self, options: RunOptions, shutdown: impl Future<Output = ()>) -> Stopped {
        let concurrency = usize::try_from(options.concurrency.get()).unwrap_or(usize::MAX);
        let mut shutdown = pin!(shutdown);
        let mut running = JoinSet::new();
        loop {
            // A stop that came while a job ended or the poll slept is seen here, before any
            // further claim starts.
            tokio::select! {
                biased;
                () = &mut shutdown => break,
                () = std::future::ready(()) => {}
            }
            self.fill(&mut running, concurrency).await;
            let full = running.len() >= concurrency;
            tokio::select! {
                biased;
                () = &mut shutdown => break,
                Some(joined) = running.join_next(), if !running.is_empty() => report_ended(joined),
                () = tokio::time::sleep(options.poll), if !full => {}
            }
        }
        tracing::info!(running = running.len(), "job worker stopping");
        drain(running, options.grace).await
    }

    /// Claim as many due jobs as `running` has room for, and start them.
    async fn fill(&self, running: &mut JoinSet<()>, concurrency: usize) {
        let room = concurrency.saturating_sub(running.len());
        if room == 0 {
            return;
        }
        // `room` is at most `concurrency`, which came from a `u32`.
        let limit = u32::try_from(room).unwrap_or(u32::MAX);
        match self.claim_due(limit).await {
            Ok(claimed) => {
                for job in claimed {
                    self.spawn(running, job);
                }
            }
            Err(error) => tracing::error!(%error, "could not claim jobs"),
        }
    }
}

/// Wait up to `grace` for every task of `running`; leave the rest running detached.
async fn drain(mut running: JoinSet<()>, grace: Duration) -> Stopped {
    let all = async {
        while let Some(joined) = running.join_next().await {
            report_ended(joined);
        }
    };
    if tokio::time::timeout(grace, all).await.is_ok() {
        tracing::info!("job worker stopped");
        return Stopped::Drained;
    }
    let left = running.len();
    running.detach_all();
    tracing::warn!(
        running = left,
        "job worker stopped with jobs still running; their leases will run out"
    );
    Stopped::GraceExpired { running: left }
}
