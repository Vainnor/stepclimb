// SPDX-License-Identifier: AGPL-3.0-or-later
//! `jobs.prune` (docs/specs/m1/01-job-queue-and-outbox.md B8, B18, Q3): every hour, delete the
//! dead jobs that died 30 days ago or more. Dispatched outbox rows join it with the outbox
//! (issue I4).
//!
//! An instance job: it walks the organizations through `organizations::ids` and deletes each
//! one's dead jobs with that organization's scope, then the instance's own (B10, B26), in
//! batches of [`BATCH`]. A failure in one organization is logged and the others still run; the
//! job retries if any failed. Safe to run twice: a second run finds nothing old enough left
//! (B12).

use std::time::Duration;

use stepclimb_core::{OrgScope, id::Uuid, organizations};

use crate::kind::Kind;
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

/// How many dead jobs one delete statement removes at most, so a large backlog never holds one
/// long transaction.
pub const BATCH: u32 = 1000;

async fn prune(ctx: JobContext) -> Outcome {
    // A clock reading before 1970-01-31 is a broken clock, not a state a retry can fix.
    let Some(cutoff) = ctx.clock().now().checked_sub(DEAD_JOB_RETENTION) else {
        return Outcome::fail("the clock reads a time less than 30 days after the Unix epoch");
    };
    let pool = ctx.pool();
    let orgs = match organizations::ids(pool).await {
        Ok(orgs) => orgs,
        Err(error) => return Outcome::retry(format_args!("could not list organizations: {error}")),
    };
    let (mut deleted, mut failed) = (0, 0_usize);
    for org_id in orgs {
        let scope = prune_scope(org_id);
        match in_batches(|| queue::prune_dead(pool, scope, cutoff, BATCH)).await {
            Ok(count) => deleted += count,
            Err(error) => {
                failed += 1;
                tracing::error!(%org_id, %error, "could not prune the organization's dead jobs");
            }
        }
    }
    match in_batches(|| queue::prune_dead_instance(pool, cutoff, BATCH)).await {
        Ok(count) => deleted += count,
        Err(error) => {
            failed += 1;
            tracing::error!(%error, "could not prune the instance's dead jobs");
        }
    }
    if deleted > 0 {
        tracing::info!(deleted, "pruned dead jobs");
    }
    // As the sign-in flow cleanup does: one organization's failure does not stop the others,
    // and the job runs again if any failed.
    if failed > 0 {
        return Outcome::retry(format_args!("could not prune dead jobs in {failed} places"));
    }
    Outcome::Done
}

/// Run `batch` until it deletes fewer than [`BATCH`] rows; the total it deleted.
async fn in_batches<F, Fut>(mut batch: F) -> Result<u64, queue::Error>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<u64, queue::Error>>,
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
