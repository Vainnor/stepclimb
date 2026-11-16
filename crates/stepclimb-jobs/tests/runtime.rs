// SPDX-License-Identifier: AGPL-3.0-or-later
//! The worker loop against a fresh database per test (docs/specs/m1/01-job-queue-and-outbox.md
//! B14, B15, Q2; T12). The poll interval and the grace period are shortened through
//! `RunOptions`; the defaults are checked on their own. Needs `DATABASE_URL`.

use std::future::Future;
use std::num::NonZeroU32;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope,
    clock::ManualClock,
    id::{Uuid, new_id},
};
use stepclimb_jobs::{
    Enqueued, Kind, NewJob, Outcome, POLL_INTERVAL, Registry, RunOptions, SHUTDOWN_GRACE, Stopped,
    Worker, backoff::Random, enqueue,
};
use tokio::sync::{Notify, Semaphore, oneshot};
use tokio::task::JoinHandle;

/// Waits for a permit from `Shared::gate`, then returns done.
const BLOCK: Kind = Kind::new("test.block", "default");
/// Returns done at once.
const DONE: Kind = Kind::new("test.done", "default");

/// How long a test waits for something the worker does before it fails.
const PATIENCE: Duration = Duration::from_secs(10);

#[derive(Clone)]
struct Shared {
    gate: Arc<Semaphore>,
    started: Arc<AtomicUsize>,
}

fn registry(shared: &Shared) -> Registry {
    let mut registry = Registry::new();
    let s = shared.clone();
    registry
        .register(BLOCK, move |_| {
            let s = s.clone();
            async move {
                s.started.fetch_add(1, Ordering::SeqCst);
                s.gate
                    .acquire()
                    .await
                    .expect("the gate stays open")
                    .forget();
                Outcome::Done
            }
        })
        .expect("register test.block");
    registry
        .register(DONE, |_| async { Outcome::Done })
        .expect("register test.done");
    registry
}

/// 2025-11-13T02:13:20Z.
fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the enqueuing request's credential scope"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

struct World {
    pool: PgPool,
    org: Uuid,
    shared: Shared,
    clock: ManualClock,
    worker: Worker,
}

async fn world(pool: PgPool) -> World {
    let org = new_id();
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, 'org-a', 'org-a')")
        .bind(org)
        .execute(&pool)
        .await
        .expect("insert organization");
    let shared = Shared {
        gate: Arc::new(Semaphore::new(0)),
        started: Arc::new(AtomicUsize::new(0)),
    };
    let clock = ManualClock::new(t0());
    let worker = Worker::new(
        pool.clone(),
        registry(&shared),
        clock.clock(),
        Random::from_fn(|| 0),
        vec!["default"],
    );
    World {
        pool,
        org,
        shared,
        clock,
        worker,
    }
}

async fn put(w: &World, kind: &Kind) -> Uuid {
    put_at(w, kind, t0()).await
}

/// Enqueue a job of `kind` due at `run_at`.
async fn put_at(w: &World, kind: &Kind, run_at: SystemTime) -> Uuid {
    let mut tx = w.pool.begin().await.expect("begin");
    let job = NewJob::org(kind, scope(w.org), &json!({}))
        .expect("a job")
        .run_at(run_at);
    let Enqueued::Inserted(id) = enqueue(&mut tx, job, t0()).await.expect("enqueue") else {
        panic!("a new row")
    };
    tx.commit().await.expect("commit");
    id
}

/// The job's `attempts` and whether it holds a lease, or `None` once its row is gone.
async fn state(pool: &PgPool, id: Uuid) -> Option<(i32, bool)> {
    sqlx::query_as("SELECT attempts, locked_until IS NOT NULL FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .expect("read job")
}

fn options(concurrency: u32) -> RunOptions {
    RunOptions {
        concurrency: NonZeroU32::new(concurrency).expect("a positive concurrency"),
        poll: Duration::from_millis(20),
        grace: Duration::from_secs(5),
    }
}

/// The stop future a test hands the worker loop: it counts how often the loop has looked at
/// it, and says when the loop has seen the stop.
struct Probe {
    stop: oneshot::Receiver<()>,
    looks: Arc<AtomicUsize>,
    seen: Arc<Notify>,
}

impl Future for Probe {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.looks.fetch_add(1, Ordering::SeqCst);
        match Pin::new(&mut self.stop).poll(cx) {
            Poll::Ready(_) => {
                self.seen.notify_one();
                Poll::Ready(())
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// A started worker loop: send on `stop` to stop it.
struct Running {
    stop: oneshot::Sender<()>,
    looks: Arc<AtomicUsize>,
    seen: Arc<Notify>,
    handle: JoinHandle<Stopped>,
}

/// Start `w`'s worker loop with `options`.
fn start(w: &World, options: RunOptions) -> Running {
    let (stop, rx) = oneshot::channel::<()>();
    let (looks, seen) = (Arc::new(AtomicUsize::new(0)), Arc::new(Notify::new()));
    let probe = Probe {
        stop: rx,
        looks: Arc::clone(&looks),
        seen: Arc::clone(&seen),
    };
    let worker = w.worker.clone();
    let handle = tokio::spawn(async move { worker.run(options, probe).await });
    Running {
        stop,
        looks,
        seen,
        handle,
    }
}

/// Wait until `check` holds, failing after [`PATIENCE`].
async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while !check().await {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn started(w: &World, n: usize) {
    let counter = Arc::clone(&w.shared.started);
    eventually(&format!("{n} handlers started"), || {
        let counter = Arc::clone(&counter);
        async move { counter.load(Ordering::SeqCst) >= n }
    })
    .await;
}

#[test]
fn the_defaults_are_the_specs() {
    assert_eq!(POLL_INTERVAL, Duration::from_secs(1));
    assert_eq!(SHUTDOWN_GRACE, Duration::from_secs(30));
    let defaults = RunOptions::new(NonZeroU32::new(4).expect("4"));
    assert_eq!(defaults.poll, POLL_INTERVAL);
    assert_eq!(defaults.grace, SHUTDOWN_GRACE);
}

// B14: no more than `concurrency` jobs at once, counting the ones already running; a finished
// one makes room for the next.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_worker_runs_at_most_its_concurrency_at_once(pool: PgPool) {
    let w = world(pool).await;
    let first = put(&w, &BLOCK).await;
    let Running { stop, handle, .. } = start(&w, options(2));
    started(&w, 1).await;
    // One slot is taken; two more jobs arrive, and the next poll may claim only one of them.
    let jobs = [first, put(&w, &BLOCK).await, put(&w, &BLOCK).await];
    started(&w, 2).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(w.shared.started.load(Ordering::SeqCst), 2);
    let mut claimed = 0;
    for id in jobs {
        claimed += i32::from(state(&w.pool, id).await.is_some_and(|(a, _)| a == 1));
    }
    assert_eq!(claimed, 2, "the third job waits unclaimed");

    w.shared.gate.add_permits(1);
    started(&w, 3).await;
    w.shared.gate.add_permits(2);
    for id in jobs {
        let pool = w.pool.clone();
        eventually("every job done", || {
            let pool = pool.clone();
            async move { state(&pool, id).await.is_none() }
        })
        .await;
    }
    stop.send(()).expect("the worker listens");
    assert_eq!(handle.await.expect("the loop ends"), Stopped::Drained);
}

// B14, Q2: an idle worker looks again every poll interval. The job becomes due only after the
// loop has looked at its stop at least four times: the first fill, its wait, and one more fill
// and wait that only the poll interval could have started, since nothing is running. It is
// then claimed on a later poll.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_idle_worker_picks_up_a_new_job_on_its_next_poll(pool: PgPool) {
    let w = world(pool).await;
    let due = t0() + Duration::from_secs(1);
    let id = put_at(&w, &DONE, due).await;
    let Running {
        stop,
        looks,
        handle,
        ..
    } = start(&w, options(4));
    eventually("the loop polled while idle", || {
        let looks = Arc::clone(&looks);
        async move { looks.load(Ordering::SeqCst) >= 4 }
    })
    .await;
    assert_eq!(state(&w.pool, id).await, Some((0, false)), "not due yet");
    w.clock.set(due);
    let pool = w.pool.clone();
    eventually("the job done", || {
        let pool = pool.clone();
        async move { state(&pool, id).await.is_none() }
    })
    .await;
    stop.send(()).expect("the worker listens");
    assert_eq!(handle.await.expect("the loop ends"), Stopped::Drained);
}

// B15: a stop given before the loop starts lets it claim nothing at all.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_worker_stopped_before_it_starts_claims_nothing(pool: PgPool) {
    let w = world(pool).await;
    let id = put(&w, &DONE).await;
    let Running { stop, handle, .. } = start(&w, options(4));
    stop.send(()).expect("the worker listens");
    let stopped = tokio::time::timeout(PATIENCE, handle)
        .await
        .expect("the loop ends")
        .expect("the loop does not panic");
    assert_eq!(stopped, Stopped::Drained);
    assert_eq!(state(&w.pool, id).await, Some((0, false)), "never claimed");
}

// T12 (B15): a stopping worker returns only once its running job ends. The job is released
// after the loop has seen the stop, so the loop is waiting on it then.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_stopping_worker_waits_for_its_running_job(pool: PgPool) {
    let w = world(pool).await;
    let running = put(&w, &BLOCK).await;
    let Running {
        stop, seen, handle, ..
    } = start(&w, options(4));
    started(&w, 1).await;
    stop.send(()).expect("the worker listens");
    seen.notified().await;
    assert!(!handle.is_finished(), "the job is still blocked");
    w.shared.gate.add_permits(1);
    let stopped = tokio::time::timeout(PATIENCE, handle)
        .await
        .expect("the loop ends once the job does")
        .expect("the loop does not panic");
    assert_eq!(stopped, Stopped::Drained);
    assert_eq!(
        state(&w.pool, running).await,
        None,
        "the running job finished"
    );
}

// T12 (B5, B15): a job still running when the grace period ends keeps its lease.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_job_still_running_after_the_grace_period_keeps_its_lease(pool: PgPool) {
    let w = world(pool).await;
    let running = put(&w, &BLOCK).await;
    let grace = Duration::from_millis(300);
    let Running { stop, handle, .. } = start(
        &w,
        RunOptions {
            grace,
            ..options(4)
        },
    );
    started(&w, 1).await;
    let asked = tokio::time::Instant::now();
    stop.send(()).expect("the worker listens");
    let stopped = tokio::time::timeout(PATIENCE, handle)
        .await
        .expect("the loop ends after the grace period")
        .expect("the loop does not panic");
    assert!(asked.elapsed() >= grace, "it waited the grace period");
    assert_eq!(stopped, Stopped::GraceExpired { running: 1 });
    assert_eq!(state(&w.pool, running).await, Some((1, true)));
}
