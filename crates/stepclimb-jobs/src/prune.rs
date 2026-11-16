// SPDX-License-Identifier: AGPL-3.0-or-later
//! `jobs.prune` (docs/specs/m1/01-job-queue-and-outbox.md B8, B18, B23, Q3): every hour, delete
//! the dead jobs that died 30 days ago or more, and the outbox events dispatched 30 days ago or
//! more. An undispatched event is never deleted.
//!
//! An instance job: it walks the organizations through `organizations::ids` and deletes each
//! one's dead jobs and dispatched events with that organization's scope, then the instance's own
//! dead jobs (B10, B26), in batches of [`BATCH`]. It also logs at `warn` the undispatched events
//! of topics this release does not know ([`outbox::warn_unknown_topics`]). A failure in one
//! organization is logged and the others still run; the job retries if any failed. Safe to run
//! twice: a second run finds nothing old enough left (B12).

use std::time::{Duration, SystemTime};

use stepclimb_core::{OrgScope, id::Uuid, organizations};

use crate::kind::Kind;
use crate::outbox::{self, EVENT_RETENTION};
use crate::queue;
use crate::registry::{Registry, RegistryError};
use crate::worker::{JobContext, Outcome};

/// The kind.
pub const PRUNE: Kind = Kind::new("jobs.prune", "default");

/// How often it runs (B18).
pub const INTERVAL: Duration = Duration::from_secs(60 * 60);

/// How long a dead job is kept after `dead_at` (B8, Q3).
pub const DEAD_JOB_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Register `jobs.prune` as a periodic kind of `registry`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register_periodic(PRUNE, INTERVAL, prune)
}

/// How many rows one delete statement removes at most, so a large backlog never holds one
/// long transaction.
pub const BATCH: u32 = 1000;

async fn prune(ctx: JobContext) -> Outcome {
    let now = ctx.clock().now();
    // A clock reading before 1970-01-31 is a broken clock, not a state a retry can fix.
    let (Some(dead_cutoff), Some(event_cutoff)) = (
        now.checked_sub(DEAD_JOB_RETENTION),
        now.checked_sub(EVENT_RETENTION),
    ) else {
        return Outcome::fail("the clock reads a time less than 30 days after the Unix epoch");
    };
    let pool = ctx.pool();
    let orgs = match organizations::ids(pool).await {
        Ok(orgs) => orgs,
        Err(error) => return Outcome::retry(format_args!("could not list organizations: {error}")),
    };
    let mut total = Pruned::default();
    for org_id in orgs {
        total.add(prune_org(pool, org_id, dead_cutoff, event_cutoff).await);
    }
    total.add(prune_instance(pool, dead_cutoff).await);
    if let Err(error) = outbox::warn_unknown_topics(pool, ctx.registry()).await {
        total.failed += 1;
        tracing::error!(%error, "could not count the events of unknown topics");
    }
    let Pruned {
        jobs,
        events,
        failed,
    } = total;
    if jobs > 0 || events > 0 {
        tracing::info!(jobs, events, "pruned dead jobs and dispatched events");
    }
    // As the sign-in flow cleanup does: one organization's failure does not stop the others,
    // and the job runs again if any failed.
    if failed > 0 {
        return Outcome::retry(format_args!("could not prune in {failed} places"));
    }
    Outcome::Done
}

/// What a run deleted, and how many of its deletes failed.
#[derive(Default)]
struct Pruned {
    jobs: u64,
    events: u64,
    failed: usize,
}

impl Pruned {
    fn add(&mut self, other: Self) {
        self.jobs += other.jobs;
        self.events += other.events;
        self.failed += other.failed;
    }
}

/// Delete the instance's own dead jobs that died at or before `dead_cutoff`.
async fn prune_instance(pool: &sqlx::PgPool, dead_cutoff: SystemTime) -> Pruned {
    let mut pruned = Pruned::default();
    match in_batches(|| queue::prune_dead_instance(pool, dead_cutoff, BATCH)).await {
        Ok(count) => pruned.jobs = count,
        Err(error) => {
            pruned.failed += 1;
            tracing::error!(%error, "could not prune the instance's dead jobs");
        }
    }
    pruned
}

/// Delete the dead jobs of `org_id` that died at or before `dead_cutoff` and its events
/// dispatched at or before `event_cutoff`, with the organization's scope.
async fn prune_org(
    pool: &sqlx::PgPool,
    org_id: Uuid,
    dead_cutoff: SystemTime,
    event_cutoff: SystemTime,
) -> Pruned {
    let scope = prune_scope(org_id);
    let mut pruned = Pruned::default();
    match in_batches(|| queue::prune_dead(pool, scope, dead_cutoff, BATCH)).await {
        Ok(count) => pruned.jobs = count,
        Err(error) => {
            pruned.failed += 1;
            tracing::error!(%org_id, %error, "could not prune the organization's dead jobs");
        }
    }
    match in_batches(|| outbox::prune_dispatched(pool, scope, event_cutoff, BATCH)).await {
        Ok(count) => pruned.events = count,
        Err(error) => {
            pruned.failed += 1;
            tracing::error!(%org_id, %error, "could not prune the organization's dispatched events");
        }
    }
    pruned
}

/// Run `batch` until it deletes fewer than [`BATCH`] rows; the total it deleted.
async fn in_batches<F, Fut, E>(mut batch: F) -> Result<u64, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<u64, E>>,
{
    let mut total = 0;
    loop {
        let count = batch().await?;
        total += count;
        if count < u64::from(BATCH) {
            return Ok(total);
        }
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "spec 02 B9 allows the instance scope in a job; jobs.prune acts in each organization in turn (spec m1/01 B10)"
)]
fn prune_scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}
