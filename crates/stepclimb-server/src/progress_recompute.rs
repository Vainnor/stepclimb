// SPDX-License-Identifier: AGPL-3.0-or-later
//! `progress.recompute_course` (docs/specs/m1/05-progress-and-events.md B8): after a change to a
//! course's rules that can change learners' counts, roll every learner's module and course
//! progress up again from their stored item rows.
//!
//! The content routes that change such a rule ([`crate::modules`], [`crate::items`]) call
//! [`enqueue`] in their own transaction, so the job exists exactly when the change commits. It
//! runs [`DELAY`] later with the course id as its dedupe key (spec m1/01 B3): edits to the course
//! before it starts join the waiting job, and a burst of edits runs it once. An edit while it
//! runs enqueues the next run.
//!
//! The job calls `stepclimb_core::progress::recompute_course` batch by batch, each batch of
//! [`stepclimb_core::progress::RECOMPUTE_BATCH`] learners in its own transaction with the
//! events it returns written to the outbox ([`crate::progress::emit`]). It holds its lease
//! between batches and stops once another worker has the job. A failed batch retries the whole
//! job; batches already committed are found unchanged and write nothing again.

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use stepclimb_core::{OrgScope, id::Uuid, progress};
use stepclimb_jobs::{JobContext, Kind, Lease, NewJob, Outcome, Registry, RegistryError};

use crate::error::ApiError;

/// The kind, on the `default` queue (spec 05 Data).
pub const RECOMPUTE: Kind = Kind::new("progress.recompute_course", "default");

/// How long after the change the job runs, so a burst of edits collapses into one run (B8).
pub const DELAY: Duration = Duration::from_secs(30);

/// The job's payload: the course to recompute, in the job's organization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payload {
    pub course_id: Uuid,
}

/// Enqueue the recompute of the course `course_id` of the scope's organization in `tx`, the
/// transaction of the change, to run [`DELAY`] after `now`, or join the one already waiting.
pub async fn enqueue(
    tx: &mut Transaction<'_, Postgres>,
    scope: OrgScope,
    course_id: Uuid,
    now: SystemTime,
) -> Result<(), ApiError> {
    let failed = |error: stepclimb_jobs::queue::Error| ApiError::internal(&error);
    let key = course_id.to_string();
    let job = NewJob::org(&RECOMPUTE, scope, &Payload { course_id })
        .and_then(|job| job.dedupe(&key))
        .map_err(failed)?
        .run_at(now + DELAY);
    stepclimb_jobs::enqueue(tx, job, now)
        .await
        .map_err(failed)?;
    Ok(())
}

/// Register `progress.recompute_course` in `registry`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(RECOMPUTE, handle)
}

async fn handle(ctx: JobContext) -> Outcome {
    let Some(scope) = ctx.scope() else {
        return Outcome::fail("progress.recompute_course needs an organization");
    };
    let payload = match serde_json::from_value::<Payload>(ctx.payload().clone()) {
        Ok(payload) => payload,
        Err(error) => return Outcome::fail(format_args!("the payload does not parse: {error}")),
    };
    let mut after = None;
    loop {
        let batch = batch(
            ctx.pool(),
            scope,
            payload.course_id,
            after,
            ctx.clock().now(),
        )
        .await;
        match batch {
            Ok(None) => return Outcome::Done,
            Ok(Some(next)) => after = Some(next),
            Err(error) => {
                tracing::error!(course_id = %payload.course_id, %error, "progress recompute failed");
                return Outcome::retry(format_args!(
                    "could not recompute the progress of course {}",
                    payload.course_id
                ));
            }
        }
        match ctx.extend_lease().await {
            Ok(Lease::Held) => {}
            Ok(Lease::Lost) => return Outcome::Done,
            Err(error) => {
                return Outcome::retry(format_args!("could not extend the lease: {error}"));
            }
        }
    }
}

/// Why a batch failed.
#[derive(Debug, thiserror::Error)]
enum BatchError {
    #[error(transparent)]
    Progress(#[from] progress::Error),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    /// The events could not be written; [`crate::progress::emit`] logged the cause.
    #[error("the progress events could not be written")]
    Events,
}

/// One batch in its own transaction: the learners after `after`, their events written to the
/// outbox before the commit. Returns the next batch's start, or `None` when this was the last.
async fn batch(
    pool: &PgPool,
    scope: OrgScope,
    course_id: Uuid,
    after: Option<Uuid>,
    now: SystemTime,
) -> Result<Option<Uuid>, BatchError> {
    let mut tx = pool.begin().await?;
    let recomputed = progress::recompute_course(&mut tx, scope, course_id, after, now).await?;
    crate::progress::emit(&mut tx, scope, &recomputed.events, now)
        .await
        .map_err(|_| BatchError::Events)?;
    tx.commit().await?;
    Ok(recomputed.next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kind_is_valid_and_registers_once() {
        RECOMPUTE.validate().expect("a valid kind");
        let mut registry = Registry::new();
        register(&mut registry).expect("registers");
        assert!(registry.knows(&RECOMPUTE));
        assert!(register(&mut registry).is_err());
    }

    #[test]
    fn the_payload_names_one_course() {
        let id = Uuid::from_u128(7);
        let value = serde_json::to_value(Payload { course_id: id }).unwrap();
        assert_eq!(value, serde_json::json!({ "course_id": id.to_string() }));
        let extra = serde_json::json!({ "course_id": id.to_string(), "x": 1 });
        assert!(serde_json::from_value::<Payload>(extra).is_err());
    }
}
