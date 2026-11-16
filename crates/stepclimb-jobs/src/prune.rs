// SPDX-License-Identifier: AGPL-3.0-or-later
//! `jobs.prune` (docs/specs/m1/01-job-queue-and-outbox.md B8, B18, Q3): every hour, delete the
//! dead jobs that died 30 days ago or more. Dispatched outbox rows join it with the outbox
//! (issue I4).
//!
//! An instance job: it walks the organizations through `organizations::ids` and deletes each
//! one's dead jobs with that organization's scope, then the instance's own (B10, B26). Safe to
//! run twice: a second run finds nothing old enough left (B12).

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

async fn prune(ctx: JobContext) -> Outcome {
    let Some(cutoff) = ctx.clock().now().checked_sub(DEAD_JOB_RETENTION) else {
        return Outcome::Done;
    };
    let pool = ctx.pool();
    let orgs = match organizations::ids(pool).await {
        Ok(orgs) => orgs,
        Err(error) => return Outcome::retry(format_args!("could not list organizations: {error}")),
    };
    let mut deleted = 0;
    for scope in orgs.into_iter().map(prune_scope).map(Some).chain([None]) {
        match queue::prune_dead(pool, scope, cutoff).await {
            Ok(count) => deleted += count,
            Err(error) => {
                return Outcome::retry(format_args!("could not prune dead jobs: {error}"));
            }
        }
    }
    if deleted > 0 {
        tracing::info!(deleted, "pruned dead jobs");
    }
    Outcome::Done
}

#[expect(
    clippy::disallowed_methods,
    reason = "spec 02 B9 allows the instance scope in a job; jobs.prune acts in each organization in turn (spec m1/01 B10)"
)]
fn prune_scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}
