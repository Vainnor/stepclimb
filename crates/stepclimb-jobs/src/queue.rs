// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `jobs` table (docs/specs/m1/01-job-queue-and-outbox.md B1 to B11, Data).
//!
//! [`enqueue`] inserts a job in the caller's transaction. [`claim`] takes due jobs for a worker;
//! the functions after it act on one claim, named by a [`Claim`]: the job's id, its organization
//! and the `attempts` value the claim set. A row is still a claim's own while it has that id,
//! that organization, that `attempts`, a lease and no `dead_at`; once the lease ran out and
//! another worker claimed the job, `attempts` moved on and every function here changes nothing
//! and reports [`Owned::Lost`].
//!
//! Every query on a claim filters by the claim's organization (`org_id IS NOT DISTINCT FROM`,
//! null for an instance job). [`claim`] alone is instance-wide, as the spec sets out (B26).

use std::time::{Duration, SystemTime};

use serde::Serialize;
use sqlx::{PgExecutor, Postgres, Transaction};
use stepclimb_core::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros},
    id::{Uuid, new_id},
};

use crate::kind::{Kind, KindError};

/// The largest payload, serialized, in bytes (B2).
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;

/// The longest `last_error`, in bytes (B9).
pub const MAX_ERROR_BYTES: usize = 2048;

/// The longest `dedupe_key`, in characters, as the column's check allows.
const MAX_DEDUPE_KEY_CHARS: usize = 200;

/// The partial unique index that keeps one waiting job per kind, organization and dedupe key
/// (B3).
const DEDUPE_INDEX: &str = "jobs_dedupe_waiting";

/// Why a queue call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Kind(#[from] KindError),
    /// The payload is larger than [`MAX_PAYLOAD_BYTES`] serialized: a programming error, since
    /// a job that needs more stores it in a table and passes the id.
    #[error("the job payload is {0} bytes serialized, more than the {MAX_PAYLOAD_BYTES} allowed")]
    PayloadTooLarge(usize),
    #[error("the job payload does not serialize")]
    Payload(#[source] serde_json::Error),
    /// The dedupe key is empty, longer than 200 characters or holds NUL.
    #[error("a dedupe key must be 1 to {MAX_DEDUPE_KEY_CHARS} characters without NUL")]
    DedupeKeyInvalid,
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// A job to enqueue.
#[derive(Clone, Debug)]
pub struct NewJob<'a> {
    kind: &'a Kind,
    org: Option<OrgScope>,
    payload: String,
    dedupe_key: Option<&'a str>,
    run_at: Option<SystemTime>,
}

impl<'a> NewJob<'a> {
    /// An organization's job, run with `scope`'s organization (B10).
    pub fn org(kind: &'a Kind, scope: OrgScope, payload: &impl Serialize) -> Result<Self, Error> {
        Self::build(kind, Some(scope), payload)
    }

    /// An instance job, run with no organization (B2, B10).
    pub fn instance(kind: &'a Kind, payload: &impl Serialize) -> Result<Self, Error> {
        Self::build(kind, None, payload)
    }

    fn build(
        kind: &'a Kind,
        org: Option<OrgScope>,
        payload: &impl Serialize,
    ) -> Result<Self, Error> {
        kind.validate()?;
        let payload = serde_json::to_string(payload).map_err(Error::Payload)?;
        if payload.len() > MAX_PAYLOAD_BYTES {
            return Err(Error::PayloadTooLarge(payload.len()));
        }
        Ok(Self {
            kind,
            org,
            payload,
            dedupe_key: None,
            run_at: None,
        })
    }

    /// Collapse this job into a waiting one of the same kind, organization and key (B3).
    pub fn dedupe(self, key: &'a str) -> Result<Self, Error> {
        let chars = key.chars().count();
        if chars == 0 || chars > MAX_DEDUPE_KEY_CHARS || key.contains('\0') {
            return Err(Error::DedupeKeyInvalid);
        }
        Ok(Self {
            dedupe_key: Some(key),
            ..self
        })
    }

    /// Run no earlier than `at`. Without it the job is due at once.
    #[must_use]
    pub fn run_at(self, at: SystemTime) -> Self {
        Self {
            run_at: Some(at),
            ..self
        }
    }
}

/// What [`enqueue`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enqueued {
    /// A new row with this id.
    Inserted(Uuid),
    /// A waiting job with this id and the same kind, organization and key absorbed the new one; its `run_at`
    /// is now the earlier of the two (B3).
    Merged(Uuid),
}

/// Enqueue `job` in `tx`, the caller's transaction, so the job exists exactly when the change
/// that asked for it commits (B2). `now` is the caller's clock reading: the row's `created_at`,
/// and its `run_at` unless the job sets one.
///
/// A waiting job (never claimed, or released) of the same kind, organization and dedupe key
/// absorbs this one (B3); a job of another organization never does, since the organization is
/// part of the unique index the upsert's conflict names. A job already claimed does not: the new row waits and runs after it.
pub async fn enqueue(
    tx: &mut Transaction<'_, Postgres>,
    job: NewJob<'_>,
    now: SystemTime,
) -> Result<Enqueued, Error> {
    let id = new_id();
    let now_us = epoch_micros(now)?;
    let run_at_us = job.run_at.map(epoch_micros).transpose()?.unwrap_or(now_us);
    let max_attempts = i32::try_from(job.kind.retry.max_attempts)
        .map_err(|_| KindError::Settings(job.kind.name))?;
    let row = sqlx::query!(
        r#"INSERT INTO jobs (id, org_id, queue, kind, payload, dedupe_key, run_at, max_attempts,
                             created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5::text::jsonb, $6,
                   timestamptz 'epoch' + $7::bigint * interval '1 microsecond', $8,
                   timestamptz 'epoch' + $9::bigint * interval '1 microsecond',
                   timestamptz 'epoch' + $9::bigint * interval '1 microsecond')
           ON CONFLICT (kind, org_id, dedupe_key)
               WHERE dedupe_key IS NOT NULL AND locked_until IS NULL AND dead_at IS NULL
           DO UPDATE SET run_at = LEAST(jobs.run_at, EXCLUDED.run_at),
                         updated_at = EXCLUDED.updated_at
           RETURNING id"#,
        id,
        job.org.map(OrgScope::org_id),
        job.kind.queue,
        job.kind.name,
        job.payload,
        job.dedupe_key,
        run_at_us,
        max_attempts,
        now_us,
    )
    .fetch_one(&mut **tx)
    .await?;
    Ok(if row.id == id {
        Enqueued::Inserted(id)
    } else {
        Enqueued::Merged(row.id)
    })
}

/// A job a worker has claimed, as the row stood after the claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claimed {
    pub id: Uuid,
    pub org_id: Option<Uuid>,
    pub kind: String,
    /// The payload as stored, JSON text.
    pub payload: String,
    /// The claim count including this claim: 1 on the first run.
    pub attempts: i32,
    pub max_attempts: i32,
}

impl Claimed {
    /// The claim, for the functions that act on it.
    #[must_use]
    pub fn claim(&self) -> Claim {
        Claim {
            id: self.id,
            org_id: self.org_id,
            attempts: self.attempts,
        }
    }
}

/// One claim of one job: the row is the claim's own while it still has this `attempts`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    pub id: Uuid,
    pub org_id: Option<Uuid>,
    pub attempts: i32,
}

/// Whether a call on a claim found the row still the claim's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owned {
    Held,
    /// The lease ran out and another worker claimed the job, or the row is gone.
    Lost,
}

impl Owned {
    fn from_rows(rows: u64) -> Self {
        if rows == 0 { Self::Lost } else { Self::Held }
    }
}

/// The `last_error` of a job whose lease lapsed on its last attempt.
pub const LAPSED_ON_LAST_ATTEMPT: &str = "lease lapsed on the last attempt";

/// A job [`claim`] found with a lapsed lease and no attempt left, and marked dead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lapsed {
    pub id: Uuid,
    pub org_id: Option<Uuid>,
    pub kind: String,
    pub attempts: i32,
}

/// What one [`claim`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Claims {
    /// The jobs to run now.
    pub claimed: Vec<Claimed>,
    /// The due jobs that had used every attempt, now dead.
    pub lapsed: Vec<Lapsed>,
}

/// Claim up to `limit` due jobs of `queues` at `now` (B4): rows with `run_at <= now`, no live
/// lease and not dead, in `run_at, id` order, skipping rows another claim has locked. Each gets
/// `attempts + 1` and a lease to `now` plus its kind's lease from `leases` (`(kind, lease)`
/// pairs), or [`crate::kind::DEFAULT_LEASE`] for a kind not listed.
///
/// A due row that has already been claimed `max_attempts` times is one whose lease lapsed on its
/// last attempt (its worker died, or its handler ended the process): it is marked dead with
/// [`LAPSED_ON_LAST_ATTEMPT`] instead of being claimed again (B5), and counts against `limit`.
///
/// Run it outside any other transaction: the claim commits on its own, before a handler runs,
/// so a handler's transactions never hold the queue's row locks.
pub async fn claim(
    db: impl PgExecutor<'_>,
    queues: &[&str],
    leases: &[(&str, Duration)],
    limit: u32,
    now: SystemTime,
) -> Result<Claims, Error> {
    let now_us = epoch_micros(now)?;
    let queues: Vec<String> = queues.iter().map(|&q| q.to_owned()).collect();
    let kinds: Vec<String> = leases.iter().map(|(k, _)| (*k).to_owned()).collect();
    let lease_us: Vec<i64> = leases.iter().map(|(_, l)| micros(*l)).collect();
    let default_lease_us = micros(crate::kind::DEFAULT_LEASE);
    // Instance-wide by design (B26): a worker serves every organization's jobs of its queues,
    // and the instance's. Each claimed job then runs with its own organization's scope (B10).
    let rows = sqlx::query!(
        r#"WITH now AS (
               SELECT timestamptz 'epoch' + $1::bigint * interval '1 microsecond' AS t
           ),
           due AS (
               SELECT j.id, j.attempts >= j.max_attempts AS exhausted FROM jobs j, now
               WHERE j.queue = ANY($2::text[])
                 AND j.dead_at IS NULL
                 AND j.run_at <= now.t
                 AND (j.locked_until IS NULL OR j.locked_until <= now.t)
               ORDER BY j.run_at, j.id
               LIMIT $3
               FOR UPDATE OF j SKIP LOCKED
           ),
           leases AS (
               SELECT * FROM unnest($4::text[], $5::bigint[]) AS l (kind, lease_us)
           ),
           lapsed AS (
               UPDATE jobs j
               SET dead_at = now.t, locked_until = NULL, last_error = $7, updated_at = now.t
               FROM due, now
               WHERE j.id = due.id AND due.exhausted
               RETURNING j.id, j.org_id, j.kind, j.attempts, j.max_attempts
           ),
           claimed AS (
               UPDATE jobs j
               SET attempts = j.attempts + 1,
                   locked_until = now.t + coalesce(
                       (SELECT l.lease_us FROM leases l WHERE l.kind = j.kind), $6::bigint
                   ) * interval '1 microsecond',
                   updated_at = now.t
               FROM due, now
               WHERE j.id = due.id AND NOT due.exhausted
               RETURNING j.id, j.org_id, j.kind, j.payload::text AS payload, j.attempts,
                         j.max_attempts
           )
           SELECT id AS "id!", org_id, kind AS "kind!", payload, attempts AS "attempts!",
                  max_attempts AS "max_attempts!", false AS "lapsed!"
           FROM claimed
           UNION ALL
           SELECT id, org_id, kind, NULL, attempts, max_attempts, true
           FROM lapsed"#,
        now_us,
        &queues,
        i64::from(limit),
        &kinds,
        &lease_us,
        default_lease_us,
        LAPSED_ON_LAST_ATTEMPT,
    )
    .fetch_all(db)
    .await?;
    let mut claims = Claims::default();
    for row in rows {
        if row.lapsed {
            claims.lapsed.push(Lapsed {
                id: row.id,
                org_id: row.org_id,
                kind: row.kind,
                attempts: row.attempts,
            });
        } else {
            claims.claimed.push(Claimed {
                id: row.id,
                org_id: row.org_id,
                kind: row.kind,
                // `jobs.payload` is NOT NULL, so a claimed row always has one; only the lapsed
                // half of the union selects NULL.
                payload: row.payload.unwrap_or_default(),
                attempts: row.attempts,
                max_attempts: row.max_attempts,
            });
        }
    }
    Ok(claims)
}

/// Move the claim's lease to `now + lease` (B5), while the row is still the claim's own.
pub async fn extend_lease(
    db: impl PgExecutor<'_>,
    claim: Claim,
    lease: Duration,
    now: SystemTime,
) -> Result<Owned, Error> {
    let now_us = epoch_micros(now)?;
    let done = sqlx::query!(
        "UPDATE jobs
         SET locked_until = timestamptz 'epoch' + ($4::bigint + $5::bigint) * interval '1 microsecond',
             updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
         WHERE id = $1 AND org_id IS NOT DISTINCT FROM $2 AND attempts = $3
           AND locked_until IS NOT NULL AND dead_at IS NULL",
        claim.id,
        claim.org_id,
        claim.attempts,
        now_us,
        micros(lease),
    )
    .execute(db)
    .await?;
    Ok(Owned::from_rows(done.rows_affected()))
}

/// Delete a finished job (B6 done), while the row is still the claim's own.
pub async fn complete(db: impl PgExecutor<'_>, claim: Claim) -> Result<Owned, Error> {
    let done = sqlx::query!(
        "DELETE FROM jobs
         WHERE id = $1 AND org_id IS NOT DISTINCT FROM $2 AND attempts = $3
           AND locked_until IS NOT NULL AND dead_at IS NULL",
        claim.id,
        claim.org_id,
        claim.attempts,
    )
    .execute(db)
    .await?;
    Ok(Owned::from_rows(done.rows_affected()))
}

/// Mark the job dead (B6 fail, B8) with `error` as its `last_error`, cut to
/// [`MAX_ERROR_BYTES`], while the row is still the claim's own. Its lease is cleared.
pub async fn kill(
    db: impl PgExecutor<'_>,
    claim: Claim,
    error: &str,
    now: SystemTime,
) -> Result<Owned, Error> {
    let now_us = epoch_micros(now)?;
    let done = sqlx::query!(
        "UPDATE jobs
         SET dead_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond',
             locked_until = NULL, last_error = $5,
             updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
         WHERE id = $1 AND org_id IS NOT DISTINCT FROM $2 AND attempts = $3
           AND locked_until IS NOT NULL AND dead_at IS NULL",
        claim.id,
        claim.org_id,
        claim.attempts,
        now_us,
        stored_error(error),
    )
    .execute(db)
    .await?;
    Ok(Owned::from_rows(done.rows_affected()))
}

/// What [`release`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Released {
    /// The job waits again until its new `run_at`.
    Waiting,
    /// A waiting job of the same kind and dedupe key covers this one, so its row was deleted
    /// (spec Data).
    Covered,
    /// The row was no longer the claim's own.
    Lost,
}

/// Clear the claim's lease, set `last_error` to `error`, cut to [`MAX_ERROR_BYTES`], and let
/// the job wait until `run_at` (B6 retry), while the row is still the claim's own.
pub async fn retry(
    db: &sqlx::PgPool,
    claim: Claim,
    run_at: SystemTime,
    error: &str,
    now: SystemTime,
) -> Result<Released, Error> {
    release(db, claim, run_at, Some(&stored_error(error)), 0, now).await
}

/// Release a claim of a kind this binary does not know (B11): clear the lease and let the job
/// wait until `run_at` without counting the attempt, so a release that knows the kind runs it
/// with its full allowance. `last_error` is left as it is.
pub async fn release_unknown(
    db: &sqlx::PgPool,
    claim: Claim,
    run_at: SystemTime,
    now: SystemTime,
) -> Result<Released, Error> {
    release(db, claim, run_at, None, 1, now).await
}

/// Clearing the lease puts the row back under `jobs_dedupe_waiting`; when a waiting job of the
/// same kind and key exists, the update fails on that index and the row is deleted instead, since
/// the waiting job covers it (spec Data). Each statement commits on its own.
async fn release(
    db: &sqlx::PgPool,
    claim: Claim,
    run_at: SystemTime,
    error: Option<&str>,
    uncount: i32,
    now: SystemTime,
) -> Result<Released, Error> {
    let now_us = epoch_micros(now)?;
    let updated = sqlx::query!(
        "UPDATE jobs
         SET locked_until = NULL,
             run_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond',
             attempts = attempts - $5::integer,
             last_error = coalesce($6, last_error),
             updated_at = timestamptz 'epoch' + $7::bigint * interval '1 microsecond'
         WHERE id = $1 AND org_id IS NOT DISTINCT FROM $2 AND attempts = $3
           AND locked_until IS NOT NULL AND dead_at IS NULL",
        claim.id,
        claim.org_id,
        claim.attempts,
        epoch_micros(run_at)?,
        uncount,
        error,
        now_us,
    )
    .execute(db)
    .await;
    match updated {
        Ok(done) if done.rows_affected() == 0 => Ok(Released::Lost),
        Ok(_) => Ok(Released::Waiting),
        Err(error) if is_dedupe_conflict(&error) => match complete(db, claim).await? {
            Owned::Held => Ok(Released::Covered),
            Owned::Lost => Ok(Released::Lost),
        },
        Err(error) => Err(error.into()),
    }
}

fn is_dedupe_conflict(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|e| e.constraint())
        .is_some_and(|c| c == DEDUPE_INDEX)
}

/// Delete up to `limit` dead jobs of `scope`'s organization that died at or before `cutoff`
/// (B8: `jobs.prune`). Returns how many it deleted; fewer than `limit` means none is left.
pub async fn prune_dead(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    cutoff: SystemTime,
    limit: u32,
) -> Result<u64, Error> {
    // `jobs_dead` serves the inner select: `org_id = $1`, `dead_at <= $2`.
    let done = sqlx::query!(
        "DELETE FROM jobs
         WHERE id IN (
             SELECT id FROM jobs
             WHERE org_id = $1 AND dead_at IS NOT NULL
               AND dead_at <= timestamptz 'epoch' + $2::bigint * interval '1 microsecond'
             LIMIT $3
         )",
        scope.org_id(),
        epoch_micros(cutoff)?,
        i64::from(limit),
    )
    .execute(db)
    .await?;
    Ok(done.rows_affected())
}

/// [`prune_dead`] for the instance's own dead jobs, those with no organization.
pub async fn prune_dead_instance(
    db: impl PgExecutor<'_>,
    cutoff: SystemTime,
    limit: u32,
) -> Result<u64, Error> {
    // Instance jobs only (`org_id IS NULL`); `jobs_dead` serves the inner select.
    let done = sqlx::query!(
        "DELETE FROM jobs
         WHERE id IN (
             SELECT id FROM jobs
             WHERE org_id IS NULL AND dead_at IS NOT NULL
               AND dead_at <= timestamptz 'epoch' + $1::bigint * interval '1 microsecond'
             LIMIT $2
         )",
        epoch_micros(cutoff)?,
        i64::from(limit),
    )
    .execute(db)
    .await?;
    Ok(done.rows_affected())
}

/// `error` as `last_error` stores it (B9): NUL, which PostgreSQL `text` cannot hold, becomes
/// U+FFFD, and the result is cut to at most [`MAX_ERROR_BYTES`] at a character boundary.
#[must_use]
pub fn stored_error(error: &str) -> String {
    let cleaned = error.replace('\0', "\u{FFFD}");
    let end = cleaned.floor_char_boundary(MAX_ERROR_BYTES);
    cleaned[..end].to_owned()
}

/// `duration` in whole microseconds.
fn micros(duration: Duration) -> i64 {
    // A lease is at most 24 hours (`Kind::validate`), about 8.6e10 µs, far below `i64::MAX`;
    // saturating keeps a lease the registry never checked in range rather than wrapping.
    i64::try_from(duration.as_micros()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_error_is_kept_whole() {
        assert_eq!(
            stored_error("the mail server refused"),
            "the mail server refused"
        );
    }

    #[test]
    fn a_long_error_is_cut_at_a_character_boundary() {
        // 2047 ASCII bytes, then a 3-byte character across the 2,048-byte limit.
        let error = format!("{}€{}", "a".repeat(2047), "b".repeat(10 * 1024));
        let stored = stored_error(&error);
        assert_eq!(stored.len(), 2047);
        assert!(stored.bytes().all(|b| b == b'a'));
        let exact = "é".repeat(1024);
        assert_eq!(stored_error(&format!("{exact}x")), exact);
    }

    #[test]
    fn nul_is_replaced() {
        assert_eq!(stored_error("a\0b"), "a\u{FFFD}b");
    }
}
