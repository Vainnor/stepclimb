// SPDX-License-Identifier: AGPL-3.0-or-later
//! Periodic jobs (docs/specs/m1/01-job-queue-and-outbox.md B17, B18).
//!
//! Each periodic kind of the [`Registry`] has one `job_schedules` row, named after the kind.
//! [`sync`] writes those rows at startup. [`tick`], which every worker process runs every
//! [`TICK`] through [`run`], takes a transaction-scoped advisory lock, so at most one process
//! ticks at a time; it enqueues one instance job for each row that is due, with the row's name
//! as its dedupe key, and moves the row's `next_run_at` on by its interval. A process that does
//! not get the lock skips the tick.
//!
//! `job_schedules` holds no organization's data, so its queries are instance-wide, like those of
//! `plans` (B26).

use std::future::Future;
use std::pin::pin;
use std::time::{Duration, SystemTime};

use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};
use stepclimb_core::{clock::Clock, db::epoch_micros};

use crate::kind::Kind;
use crate::queue::{Error, NewJob, enqueue};
use crate::registry::Registry;

/// How often a worker process's scheduler ticks (B17).
pub const TICK: Duration = Duration::from_secs(10);

/// The advisory lock every scheduler takes for its tick: the ASCII bytes of `STEPSCHD`. Only
/// the scheduler uses it.
pub const LOCK_KEY: i64 = 0x5354_4550_5343_4844;

/// What one [`tick`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tick {
    /// Another process held the lock; nothing was read or written.
    Skipped,
    /// The names of the schedules it enqueued a job for, sorted.
    Ran(Vec<String>),
}

/// Write a schedule row for every periodic kind of `registry` (B17): a new kind gets one due at
/// `now`, and a row whose interval differs from the code's takes the code's, with its
/// `next_run_at` brought forward to `now` plus the new interval when that is sooner, so a
/// shortened interval takes effect at once. A longer interval leaves `next_run_at` as it is.
/// Rows of kinds this binary does not know, a newer release's, are left alone.
///
/// During a rolling upgrade an older binary that starts after a newer one writes its own
/// interval back; the last process to start wins until every process runs the new release.
pub async fn sync(pool: &PgPool, registry: &Registry, now: SystemTime) -> Result<(), Error> {
    let now_us = epoch_micros(now)?;
    for (name, interval) in registry.periodic() {
        // `Registry::register_periodic` keeps the interval within `i32` seconds.
        let interval_s = i32::try_from(interval.as_secs()).unwrap_or(i32::MAX);
        // Instance-wide: `job_schedules` has no organization (module docs).
        sqlx::query!(
            "INSERT INTO job_schedules (name, interval_s, next_run_at, created_at, updated_at)
             VALUES ($1, $2,
                     timestamptz 'epoch' + $3::bigint * interval '1 microsecond',
                     timestamptz 'epoch' + $3::bigint * interval '1 microsecond',
                     timestamptz 'epoch' + $3::bigint * interval '1 microsecond')
             ON CONFLICT (name) DO UPDATE
                 SET interval_s = EXCLUDED.interval_s,
                     next_run_at = LEAST(
                         job_schedules.next_run_at,
                         EXCLUDED.next_run_at + EXCLUDED.interval_s * interval '1 second'
                     ),
                     updated_at = EXCLUDED.updated_at
                 WHERE job_schedules.interval_s <> EXCLUDED.interval_s",
            name,
            interval_s,
            now_us,
        )
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// One scheduler tick at `now` (B17), in one transaction: unless another process holds
/// [`LOCK_KEY`], enqueue one instance job for each due schedule of a kind `registry` knows, and
/// move its `next_run_at` to `now` plus its interval. A due row of a kind this binary does not
/// know is left for a process that does.
pub async fn tick(pool: &PgPool, registry: &Registry, now: SystemTime) -> Result<Tick, Error> {
    let now_us = epoch_micros(now)?;
    let mut tx = pool.begin().await?;
    let locked = sqlx::query_scalar!(
        r#"SELECT pg_try_advisory_xact_lock($1) AS "locked!""#,
        LOCK_KEY
    )
    .fetch_one(&mut *tx)
    .await?;
    if !locked {
        tx.rollback().await?;
        return Ok(Tick::Skipped);
    }
    let known: Vec<String> = registry
        .periodic()
        .into_iter()
        .map(|(name, _)| name.to_owned())
        .collect();
    // Instance-wide: `job_schedules` has no organization (module docs).
    let due = sqlx::query!(
        "SELECT name, interval_s FROM job_schedules
         WHERE next_run_at <= timestamptz 'epoch' + $1::bigint * interval '1 microsecond'
           AND name = ANY($2::text[])
         ORDER BY name
         FOR UPDATE",
        now_us,
        &known,
    )
    .fetch_all(&mut *tx)
    .await?;
    let mut enqueued = Vec::with_capacity(due.len());
    for row in due {
        if let Some(kind) = registry.kind(&row.name) {
            run_due(&mut tx, kind, &row.name, row.interval_s, now).await?;
            enqueued.push(row.name);
        }
    }
    tx.commit().await?;
    Ok(Tick::Ran(enqueued))
}

/// Enqueue the instance job of the due schedule `name`, deduped by that name, and move the
/// schedule's `next_run_at` to `now` plus `interval_s` seconds, in the tick's transaction.
async fn run_due(
    tx: &mut Transaction<'_, Postgres>,
    kind: &Kind,
    name: &str,
    interval_s: i32,
    now: SystemTime,
) -> Result<(), Error> {
    let job = NewJob::instance(kind, &json!({}))?.dedupe(name)?;
    enqueue(tx, job, now).await?;
    // Instance-wide: `job_schedules` has no organization (module docs).
    sqlx::query!(
        "UPDATE job_schedules
         SET next_run_at = timestamptz 'epoch'
                 + ($2::bigint + $3::bigint * 1000000) * interval '1 microsecond',
             updated_at = timestamptz 'epoch' + $2::bigint * interval '1 microsecond'
         WHERE name = $1",
        name,
        epoch_micros(now)?,
        i64::from(interval_s),
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Tick every `every` at `clock`'s time until `shutdown` completes, the first tick at once. A
/// failed tick is logged and the next one runs on time.
pub async fn run(
    pool: PgPool,
    registry: Registry,
    clock: Clock,
    every: Duration,
    shutdown: impl Future<Output = ()>,
) {
    let mut shutdown = pin!(shutdown);
    loop {
        match tick(&pool, &registry, clock.now()).await {
            Ok(Tick::Ran(names)) if !names.is_empty() => {
                tracing::debug!(?names, "enqueued periodic jobs");
            }
            Ok(_) => {}
            Err(error) => tracing::error!(%error, "the job scheduler's tick failed"),
        }
        tokio::select! {
            biased;
            () = &mut shutdown => break,
            () = tokio::time::sleep(every) => {}
        }
    }
    tracing::info!("job scheduler stopped");
}
