// SPDX-License-Identifier: AGPL-3.0-or-later
//! Running claimed jobs (docs/specs/m1/01-job-queue-and-outbox.md B4 to B12, B25).
//!
//! [`Worker::run_once`] is one pass of a worker: claim up to `limit` due jobs, run each on a task
//! of its own, and settle each outcome. [`Worker::run`] (`crate::runtime`) is the loop a server
//! runs, with its concurrency, polling and shutdown; tests of a single pass call `run_once` and
//! move a manual clock.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use sqlx::PgPool;
use stepclimb_core::{OrgScope, clock::Clock, id::Uuid, organizations};
use tokio::task::{JoinError, JoinSet};

use crate::backoff::{Random, backoff};
use crate::kind::Kind;
use crate::queue::{self, Claim, Claimed, Error, Owned, Released};
use crate::registry::{Handler, Registry};

/// How long a job of a kind this binary does not know waits before a worker looks at it again
/// (B11).
pub const UNKNOWN_KIND_DELAY: Duration = Duration::from_secs(60);

/// What a handler reports (B6). The error text of `Retry` and `Fail` is stored as the job's
/// `last_error`: it must hold no secret, token, password or personal data beyond ids (B9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The job is finished; its row is deleted.
    Done,
    /// The job failed in a way a later run may not: it runs again after a backoff, or is dead
    /// once it has used its attempts.
    Retry(String),
    /// The job failed in a way no retry can fix; it is dead at once.
    Fail(String),
}

impl Outcome {
    /// Retry, storing `error`'s `Display`.
    pub fn retry(error: impl fmt::Display) -> Self {
        Self::Retry(error.to_string())
    }

    /// Fail, storing `error`'s `Display`.
    pub fn fail(error: impl fmt::Display) -> Self {
        Self::Fail(error.to_string())
    }
}

/// The job a handler runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub id: Uuid,
    pub kind: &'static str,
    /// The job's organization; `None` for an instance job.
    pub org_id: Option<Uuid>,
    /// Which claim this is: 1 on the first run.
    pub attempt: u32,
    pub max_attempts: u32,
}

/// What a handler gets: the job, its payload, its scope and the means to hold its lease.
#[derive(Clone, Debug)]
pub struct JobContext {
    job: Job,
    scope: Option<OrgScope>,
    payload: Value,
    pool: PgPool,
    clock: Clock,
    claim: Claim,
    lease: Duration,
    registry: Arc<Registry>,
}

/// Whether [`JobContext::extend_lease`] kept the job.
pub use crate::queue::Owned as Lease;

impl JobContext {
    #[must_use]
    pub fn job(&self) -> &Job {
        &self.job
    }

    /// The scope of the job's organization (B10); `None` for an instance job, which reads only
    /// instance tables or iterates organizations itself.
    #[must_use]
    pub fn scope(&self) -> Option<OrgScope> {
        self.scope
    }

    #[must_use]
    pub fn payload(&self) -> &Value {
        &self.payload
    }

    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    #[must_use]
    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    /// The registry the worker runs with: the kinds and event topics this binary knows.
    #[must_use]
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Hold the job for another lease from now (B5). [`Lease::Lost`] means another worker has
    /// claimed the job since; the handler should stop, since its outcome will change nothing.
    pub async fn extend_lease(&self) -> Result<Lease, Error> {
        queue::extend_lease(&self.pool, self.claim, self.lease, self.clock.now()).await
    }
}

/// Claims and runs jobs of some queues.
#[derive(Clone, Debug)]
pub struct Worker {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    pool: PgPool,
    registry: Arc<Registry>,
    clock: Clock,
    random: Random,
    queues: Vec<&'static str>,
}

impl Worker {
    /// A worker that claims jobs of `queues` and runs them with `registry`'s handlers.
    #[must_use]
    pub fn new(
        pool: PgPool,
        registry: Registry,
        clock: Clock,
        random: Random,
        queues: Vec<&'static str>,
    ) -> Self {
        let inner = Inner {
            pool,
            registry: Arc::new(registry),
            clock,
            random,
            queues,
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Claim up to `limit` due jobs, run them at once and settle their outcomes. Returns how
    /// many jobs it claimed to run; a job the claim found dead with its last lease lapsed is
    /// logged and not counted (B5, B8). A database error while settling one job is logged and
    /// leaves that job's lease to run out, so another claim runs it again (B12).
    pub async fn run_once(&self, limit: u32) -> Result<usize, Error> {
        let claimed = self.claim_due(limit).await?;
        let count = claimed.len();
        let mut running = JoinSet::new();
        for job in claimed {
            self.spawn(&mut running, job);
        }
        while let Some(joined) = running.join_next().await {
            report_ended(joined);
        }
        Ok(count)
    }

    /// Claim up to `limit` due jobs of this worker's queues, logging each the claim found dead
    /// with its last lease lapsed.
    pub(crate) async fn claim_due(&self, limit: u32) -> Result<Vec<Claimed>, Error> {
        let inner = &self.inner;
        let leases = inner.registry.leases();
        let now = inner.clock.now();
        let claims = queue::claim(&inner.pool, &inner.queues, &leases, limit, now).await?;
        for job in &claims.lapsed {
            tracing::error!(
                kind = job.kind, job_id = %job.id, org_id = %OrgField(job.org_id),
                attempts = job.attempts, error = queue::LAPSED_ON_LAST_ATTEMPT, "job dead"
            );
        }
        Ok(claims.claimed)
    }

    /// Run `job` and settle its outcome on a task of `running`.
    pub(crate) fn spawn(&self, running: &mut JoinSet<()>, job: Claimed) {
        running.spawn(process(Arc::clone(&self.inner), job));
    }
}

/// Log a job task of a [`JoinSet`] that ended other than by returning. Only that it ended: a
/// panic's message may quote a payload value (B25).
pub(crate) fn report_ended(joined: Result<(), JoinError>) {
    if let Err(error) = joined {
        let panicked = error.is_panic();
        tracing::error!(panicked, "a job's settling task ended abnormally");
    }
}

/// The `org_id` field of a job's log lines: the id, or `instance`.
struct OrgField(Option<Uuid>);

impl fmt::Display for OrgField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(id) => id.fmt(f),
            None => f.write_str("instance"),
        }
    }
}

async fn process(inner: Arc<Inner>, claimed: Claimed) {
    let org = OrgField(claimed.org_id);
    let (id, attempt) = (claimed.id, claimed.attempts);
    let Some((kind, handler)) = inner.registry.get(&claimed.kind) else {
        release_unknown(&inner, &claimed).await;
        return;
    };
    let kind_name = kind.name;
    tracing::debug!(kind = kind_name, job_id = %id, org_id = %org, attempt, "job started");
    let outcome = run_handler(&inner, &claimed, kind, Arc::clone(handler)).await;
    match settle(&inner, &claimed, kind, outcome).await {
        Ok(result) => tracing::info!(
            kind = kind_name, job_id = %id, org_id = %org, attempt, outcome = result, "job ended"
        ),
        Err(error) => tracing::error!(
            kind = kind_name, job_id = %id, org_id = %org, attempt, %error,
            "could not record the job's outcome; its lease will run out and it will run again"
        ),
    }
}

async fn release_unknown(inner: &Inner, claimed: &Claimed) {
    let org = OrgField(claimed.org_id);
    let now = inner.clock.now();
    let run_at = now + UNKNOWN_KIND_DELAY;
    let released = queue::release_unknown(&inner.pool, claimed.claim(), run_at, now).await;
    let kind = claimed.kind.as_str();
    match released {
        Ok(result) => tracing::warn!(
            kind, job_id = %claimed.id, org_id = %org, released = ?result,
            "released a job of a kind this binary does not know"
        ),
        Err(error) => tracing::error!(
            kind, job_id = %claimed.id, org_id = %org, %error,
            "could not release a job of a kind this binary does not know"
        ),
    }
}

/// The handler's outcome, after the checks that come before it (B10) and with a panic counted
/// as a retry (B6).
async fn run_handler(inner: &Inner, claimed: &Claimed, kind: &Kind, handler: Handler) -> Outcome {
    let Ok(payload) = serde_json::from_str::<Value>(&claimed.payload) else {
        return Outcome::fail("the stored payload does not parse");
    };
    let scope = match claimed.org_id {
        None => None,
        Some(org_id) => match organization_scope(&inner.pool, org_id).await {
            Ok(scope) => Some(scope),
            Err(outcome) => return outcome,
        },
    };
    let context = JobContext {
        job: Job {
            id: claimed.id,
            kind: kind.name,
            org_id: claimed.org_id,
            // `jobs` checks `attempts >= 0` and `max_attempts >= 1`, so neither conversion fails.
            attempt: u32::try_from(claimed.attempts).unwrap_or(0),
            max_attempts: u32::try_from(claimed.max_attempts).unwrap_or(0),
        },
        scope,
        payload,
        pool: inner.pool.clone(),
        clock: inner.clock.clone(),
        claim: claimed.claim(),
        lease: kind.lease,
        registry: Arc::clone(&inner.registry),
    };
    // The handler runs on a task of its own, so a panic ends that task and comes back here as
    // a `JoinError` instead of ending the worker.
    match tokio::spawn(handler(context)).await {
        Ok(outcome) => outcome,
        // The `JoinError` is not logged: a panic's message may quote a payload value (B25).
        Err(error) => {
            let panicked = error.is_panic();
            tracing::error!(
                kind = kind.name, job_id = %claimed.id, org_id = %OrgField(claimed.org_id),
                panicked, "the job's handler panicked or was cancelled"
            );
            Outcome::retry("the job's handler panicked")
        }
    }
}

/// The scope a job of `org_id` runs with, once the organization is known to exist (B10).
async fn organization_scope(pool: &PgPool, org_id: Uuid) -> Result<OrgScope, Outcome> {
    let scope = job_scope(org_id);
    match organizations::get(pool, scope).await {
        Ok(_) => Ok(scope),
        Err(organizations::Error::NotFound) => {
            Err(Outcome::fail("the job's organization does not exist"))
        }
        Err(error) => Err(Outcome::retry(format_args!(
            "could not read the job's organization: {error}"
        ))),
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "spec 02 B9 allows the instance scope in job workers; the organization comes from the job's own row, written with the enqueuer's scope (spec m1/01 B10)"
)]
fn job_scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// Record `outcome` on the job's row (B6 to B8) and name what happened, for the log.
async fn settle(
    inner: &Inner,
    claimed: &Claimed,
    kind: &Kind,
    outcome: Outcome,
) -> Result<&'static str, Error> {
    let claim = claimed.claim();
    let now = inner.clock.now();
    let error = match outcome {
        Outcome::Done => {
            return Ok(match queue::complete(&inner.pool, claim).await? {
                Owned::Held => "done",
                Owned::Lost => "done, claim lost",
            });
        }
        Outcome::Retry(error) if claimed.attempts < claimed.max_attempts => {
            // A claimed row has `attempts >= 1` (the claim added one), so this never falls back.
            let attempts = u32::try_from(claimed.attempts).unwrap_or(1);
            let run_at = now + backoff(&kind.retry, attempts, &inner.random);
            return Ok(
                match queue::retry(&inner.pool, claim, run_at, &error, now).await? {
                    Released::Waiting => "retry",
                    Released::Covered => "retry, covered by a waiting job",
                    Released::Lost => "retry, claim lost",
                },
            );
        }
        Outcome::Retry(error) | Outcome::Fail(error) => error,
    };
    Ok(match queue::kill(&inner.pool, claim, &error, now).await? {
        Owned::Held => {
            tracing::error!(
                kind = kind.name, job_id = %claim.id, org_id = %OrgField(claim.org_id),
                attempts = claim.attempts, error = %queue::stored_error(&error), "job dead"
            );
            "dead"
        }
        Owned::Lost => "dead, claim lost",
    })
}
