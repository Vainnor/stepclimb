// SPDX-License-Identifier: AGPL-3.0-or-later
//! The outbox dispatcher (docs/specs/m1/01-job-queue-and-outbox.md B21, Q2).
//!
//! [`dispatch_in`] is one pass in a transaction: claim up to [`BATCH`] undispatched events of
//! the topic and version pairs the registry knows, enqueue one job per subscriber of each event's topic with the
//! event's organization, and mark the events dispatched. The claim, the jobs and the mark commit
//! together, so each event becomes each subscriber's job exactly once; a pass that fails or is
//! cut off before its commit leaves the events undispatched for the next one. [`run`] is the
//! loop every worker process runs: a pass every poll interval when idle, and at once again after
//! a pass that found a full batch.

use std::future::Future;
use std::pin::pin;
use std::time::{Duration, SystemTime};

use sqlx::{PgPool, Postgres, Transaction};
use stepclimb_core::{OrgScope, clock::Clock, db::epoch_micros, id::Uuid};

use super::{Error, EventJob, known_pairs};
use crate::kind::Kind;
use crate::queue::{NewJob, enqueue};
use crate::registry::Registry;

/// The most events one pass claims (B21).
pub const BATCH: u32 = 100;

/// What one pass did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dispatched {
    /// How many events it marked dispatched.
    pub events: usize,
    /// How many subscriber jobs it enqueued.
    pub jobs: usize,
}

/// One pass at `now` in `tx`, claiming up to `limit` events. The caller commits `tx`; until it
/// does, another dispatcher skips the claimed events instead of waiting for them.
///
/// Only events of the topic and version pairs `registry` knows are claimed: an event of a newer
/// release's topic or version waits for a process that knows it and its subscribers. A known
/// topic with no subscriber is marked dispatched with no job.
pub async fn dispatch_in(
    tx: &mut Transaction<'_, Postgres>,
    registry: &Registry,
    limit: u32,
    now: SystemTime,
) -> Result<Dispatched, Error> {
    let topics = registry.topics();
    if topics.is_empty() {
        return Ok(Dispatched::default());
    }
    let events = claim(tx, registry, limit).await?;
    let mut jobs = 0;
    for event in &events {
        let subscribers = topics
            .iter()
            .find(|(topic, _)| topic.name == event.topic)
            .map_or(&[][..], |(_, kinds)| *kinds);
        jobs += enqueue_subscribers(tx, event, subscribers, now).await?;
    }
    mark_dispatched(tx, &events, now).await?;
    Ok(Dispatched {
        events: events.len(),
        jobs,
    })
}

/// An event a pass claimed.
struct Claimed {
    id: Uuid,
    org_id: Uuid,
    topic: String,
}

/// Lock up to `limit` undispatched events of the topic and version pairs `registry` knows, in
/// `id` order, skipping those another pass holds.
async fn claim(
    tx: &mut Transaction<'_, Postgres>,
    registry: &Registry,
    limit: u32,
) -> Result<Vec<Claimed>, Error> {
    let (names, versions) = known_pairs(registry);
    // Instance-wide by design (B26): one dispatcher serves every organization's events. Each
    // event's jobs are then enqueued with that event's own organization.
    let events = sqlx::query_as!(
        Claimed,
        "SELECT o.id, o.org_id, o.topic FROM outbox o
         WHERE o.dispatched_at IS NULL
           AND (o.topic, o.version) IN (SELECT * FROM unnest($1::text[], $2::integer[]))
         ORDER BY o.id
         LIMIT $3
         FOR UPDATE OF o SKIP LOCKED",
        &names,
        &versions,
        i64::from(limit),
    )
    .fetch_all(&mut **tx)
    .await?;
    Ok(events)
}

/// Enqueue one job of each of `subscribers` for `event`, with the event's organization and the
/// payload `{ "event_id": <id> }`. Returns how many it enqueued.
async fn enqueue_subscribers(
    tx: &mut Transaction<'_, Postgres>,
    event: &Claimed,
    subscribers: &[Kind],
    now: SystemTime,
) -> Result<usize, Error> {
    let scope = event_scope(event.org_id);
    let payload = EventJob { event_id: event.id };
    for kind in subscribers {
        enqueue(tx, NewJob::org(kind, scope, &payload)?, now).await?;
    }
    Ok(subscribers.len())
}

/// Set `dispatched_at` on the claimed `events`, each by its id and organization.
async fn mark_dispatched(
    tx: &mut Transaction<'_, Postgres>,
    events: &[Claimed],
    now: SystemTime,
) -> Result<(), Error> {
    let ids: Vec<Uuid> = events.iter().map(|e| e.id).collect();
    let orgs: Vec<Uuid> = events.iter().map(|e| e.org_id).collect();
    sqlx::query!(
        "UPDATE outbox
         SET dispatched_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond',
             updated_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
         WHERE (id, org_id) IN (SELECT * FROM unnest($1::uuid[], $2::uuid[]))",
        &ids,
        &orgs,
        epoch_micros(now)?,
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// One pass at `now` of up to [`BATCH`] events, in a transaction of its own.
pub async fn dispatch(
    pool: &PgPool,
    registry: &Registry,
    now: SystemTime,
) -> Result<Dispatched, Error> {
    let mut tx = pool.begin().await?;
    let dispatched = dispatch_in(&mut tx, registry, BATCH, now).await?;
    tx.commit().await?;
    Ok(dispatched)
}

/// Dispatch at `clock`'s time until `shutdown` completes: a pass, then another at once if it
/// found a full batch, otherwise after `poll` (B21, Q2). A failed pass is logged and the next
/// one runs after `poll`.
pub async fn run(
    pool: PgPool,
    registry: Registry,
    clock: Clock,
    poll: Duration,
    shutdown: impl Future<Output = ()>,
) {
    let mut shutdown = pin!(shutdown);
    let full = usize::try_from(BATCH).unwrap_or(usize::MAX);
    loop {
        let more = match dispatch(&pool, &registry, clock.now()).await {
            Ok(done) => {
                if done.events > 0 {
                    tracing::debug!(events = done.events, jobs = done.jobs, "dispatched events");
                }
                done.events >= full
            }
            Err(error) => {
                tracing::error!(%error, "the outbox dispatch failed");
                false
            }
        };
        tokio::select! {
            biased;
            () = &mut shutdown => break,
            () = std::future::ready(()), if more => {}
            () = tokio::time::sleep(poll), if !more => {}
        }
    }
    tracing::info!("outbox dispatcher stopped");
}

#[expect(
    clippy::disallowed_methods,
    reason = "spec 02 B9 allows the instance scope in job workers; the organization comes from the event's own row, written with the emitter's scope (spec m1/01 B19, B21)"
)]
fn event_scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}
