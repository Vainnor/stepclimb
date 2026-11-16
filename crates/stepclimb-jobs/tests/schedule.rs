// SPDX-License-Identifier: AGPL-3.0-or-later
//! Periodic jobs and `jobs.prune` against a fresh database per test
//! (docs/specs/m1/01-job-queue-and-outbox.md B17, B18, B8; T13, T6's pruning half). Needs
//! `DATABASE_URL`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope,
    clock::ManualClock,
    id::{Uuid, new_id},
};
use stepclimb_jobs::{
    Enqueued, Kind, NewJob, Outcome, Registry, Worker,
    backoff::Random,
    enqueue, prune,
    queue::{self},
    schedule::{self, LOCK_KEY, Tick},
};

const MINUTE: Duration = Duration::from_secs(60);
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// A periodic test kind, every 10 minutes.
const SWEEP: Kind = Kind::new("test.sweep", "default");
/// A second one, hourly.
const REPORT: Kind = Kind::new("test.report", "default");
/// Fails at once, to make dead jobs.
const FAIL: Kind = Kind::new("test.fail", "default");

fn registry(sweep_every: Duration) -> Registry {
    let mut registry = Registry::new();
    registry
        .register_periodic(SWEEP, sweep_every, |_| async { Outcome::Done })
        .expect("register test.sweep");
    registry
        .register_periodic(REPORT, 60 * MINUTE, |_| async { Outcome::Done })
        .expect("register test.report");
    registry
        .register(FAIL, |_| async { Outcome::fail("the course is gone") })
        .expect("register test.fail");
    prune::register(&mut registry).expect("register jobs.prune");
    registry
}

/// 2025-11-13T02:13:20Z.
fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

fn micros(time: SystemTime) -> i64 {
    i64::try_from(
        time.duration_since(UNIX_EPOCH)
            .expect("test setup")
            .as_micros(),
    )
    .expect("test setup")
}

/// Every schedule row as `(name, interval_s, next_run_at)`, by name.
async fn schedules(pool: &PgPool) -> Vec<(String, i32, i64)> {
    sqlx::query_as(
        "SELECT name, interval_s, (extract(epoch FROM next_run_at) * 1000000)::bigint
         FROM job_schedules ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .expect("read schedules")
}

/// Every job as `(kind, dedupe_key, org_id)`, by kind.
async fn jobs(pool: &PgPool) -> Vec<(String, Option<String>, Option<Uuid>)> {
    sqlx::query_as("SELECT kind, dedupe_key, org_id FROM jobs ORDER BY kind, id")
        .fetch_all(pool)
        .await
        .expect("read jobs")
}

fn ran(names: &[&str]) -> Tick {
    Tick::Ran(names.iter().map(|&n| n.to_owned()).collect())
}

// B17: a schedule row per periodic kind, due at once.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn startup_writes_a_schedule_row_for_each_periodic_kind(pool: PgPool) {
    schedule::sync(&pool, &registry(10 * MINUTE), t0())
        .await
        .unwrap();
    assert_eq!(
        schedules(&pool).await,
        [
            ("jobs.prune".to_owned(), 3600, micros(t0())),
            ("test.report".to_owned(), 3600, micros(t0())),
            ("test.sweep".to_owned(), 600, micros(t0())),
        ]
    );
}

// T13 (B17): an interval the code changed is updated; `next_run_at` and other kinds' rows stay.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn startup_updates_a_changed_interval_and_leaves_unknown_rows(pool: PgPool) {
    schedule::sync(&pool, &registry(10 * MINUTE), t0())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO job_schedules (name, interval_s, next_run_at) VALUES ('newer.kind', 60, now())",
    )
    .execute(&pool)
    .await
    .unwrap();
    let before: Vec<(String, i64)> = sqlx::query_as(
        "SELECT name, interval_s::bigint FROM job_schedules WHERE name = 'newer.kind'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let later = t0() + DAY;
    schedule::sync(&pool, &registry(5 * MINUTE), later)
        .await
        .unwrap();
    let rows = schedules(&pool).await;
    let sweep = rows.iter().find(|r| r.0 == "test.sweep").unwrap();
    assert_eq!((sweep.1, sweep.2), (300, micros(t0())), "interval only");
    let report = rows.iter().find(|r| r.0 == "test.report").unwrap();
    assert_eq!((report.1, report.2), (3600, micros(t0())), "unchanged");
    let after: Vec<(String, i64)> = sqlx::query_as(
        "SELECT name, interval_s::bigint FROM job_schedules WHERE name = 'newer.kind'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(after, before, "a newer release's row is left alone");
}

// B17: a tick enqueues one instance job per due schedule, keyed by its name, and moves it on.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_tick_enqueues_each_due_schedule_and_moves_it_on(pool: PgPool) {
    let registry = registry(10 * MINUTE);
    schedule::sync(&pool, &registry, t0()).await.unwrap();
    let tick = schedule::tick(&pool, &registry, t0()).await.unwrap();
    assert_eq!(tick, ran(&["jobs.prune", "test.report", "test.sweep"]));
    assert_eq!(
        jobs(&pool).await,
        [
            ("jobs.prune".to_owned(), Some("jobs.prune".to_owned()), None),
            (
                "test.report".to_owned(),
                Some("test.report".to_owned()),
                None
            ),
            ("test.sweep".to_owned(), Some("test.sweep".to_owned()), None),
        ]
    );
    let rows = schedules(&pool).await;
    assert_eq!(
        rows[2],
        ("test.sweep".to_owned(), 600, micros(t0() + 10 * MINUTE))
    );
    assert_eq!(
        rows[1],
        ("test.report".to_owned(), 3600, micros(t0() + 60 * MINUTE))
    );

    let just_before = t0() + 10 * MINUTE - Duration::from_micros(1);
    let tick = schedule::tick(&pool, &registry, just_before).await.unwrap();
    assert_eq!(tick, ran(&[]), "nothing due yet");
    assert_eq!(jobs(&pool).await.len(), 3);
}

// T13 (B17): two schedulers ticking at once enqueue one job per interval.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn two_schedulers_ticking_at_once_enqueue_one_job_per_interval(pool: PgPool) {
    let registry = registry(10 * MINUTE);
    schedule::sync(&pool, &registry, t0()).await.unwrap();
    let (a, b) = tokio::join!(
        schedule::tick(&pool, &registry, t0()),
        schedule::tick(&pool, &registry, t0()),
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    let sweeps = |t: &Tick| matches!(t, Tick::Ran(n) if n.iter().any(|n| n == "test.sweep"));
    assert_eq!(
        usize::from(sweeps(&a)) + usize::from(sweeps(&b)),
        1,
        "{a:?} {b:?}"
    );
    let count = |pool: PgPool| async move {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM jobs WHERE kind = 'test.sweep'")
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    assert_eq!(count(pool.clone()).await, 1);

    // The job runs; the next interval brings exactly one more.
    let clock = ManualClock::new(t0());
    let worker = Worker::new(
        pool.clone(),
        registry.clone(),
        clock.clock(),
        Random::from_fn(|| 0),
        vec!["default"],
    );
    worker.run_once(10).await.unwrap();
    assert_eq!(count(pool.clone()).await, 0);
    let next = t0() + 10 * MINUTE;
    let (a, b) = tokio::join!(
        schedule::tick(&pool, &registry, next),
        schedule::tick(&pool, &registry, next),
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(usize::from(sweeps(&a)) + usize::from(sweeps(&b)), 1);
    assert_eq!(count(pool).await, 1);
}

// B17: a tick that cannot take the lock skips, reading and writing nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_tick_that_cannot_take_the_lock_skips(pool: PgPool) {
    let registry = registry(10 * MINUTE);
    schedule::sync(&pool, &registry, t0()).await.unwrap();
    let mut holder = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(LOCK_KEY)
        .execute(&mut *holder)
        .await
        .unwrap();
    let tick = schedule::tick(&pool, &registry, t0()).await.unwrap();
    assert_eq!(tick, Tick::Skipped);
    assert!(jobs(&pool).await.is_empty());
    assert!(schedules(&pool).await.iter().all(|r| r.2 == micros(t0())));
    holder.rollback().await.unwrap();
    assert_eq!(
        schedule::tick(&pool, &registry, t0()).await.unwrap(),
        ran(&["jobs.prune", "test.report", "test.sweep"])
    );
}

// B17: a due row of a kind this binary does not know is left for a release that does.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_tick_leaves_a_due_schedule_of_an_unknown_kind(pool: PgPool) {
    sqlx::query(
        "INSERT INTO job_schedules (name, interval_s, next_run_at)
         VALUES ('newer.kind', 60, timestamptz 'epoch')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let tick = schedule::tick(&pool, &registry(10 * MINUTE), t0())
        .await
        .unwrap();
    assert_eq!(tick, ran(&[]));
    assert!(jobs(&pool).await.is_empty());
    assert_eq!(schedules(&pool).await, [("newer.kind".to_owned(), 60, 0)]);
}

// B17: the scheduler loop ticks at once, then every interval, and stops on shutdown.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_scheduler_ticks_at_once_and_stops_on_shutdown(pool: PgPool) {
    let registry = registry(10 * MINUTE);
    schedule::sync(&pool, &registry, t0()).await.unwrap();
    let clock = ManualClock::new(t0());
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let loop_ = tokio::spawn(schedule::run(
        pool.clone(),
        registry,
        clock.clock(),
        Duration::from_millis(20),
        async move {
            let _ = stopped.await;
        },
    ));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while jobs(&pool).await.len() < 3 {
        assert!(tokio::time::Instant::now() < deadline, "no first tick");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), loop_)
        .await
        .expect("the scheduler stops")
        .unwrap();
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the enqueuing request's credential scope"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// A job of `org` (or an instance job) that died at `at`.
async fn dead_job(pool: &PgPool, org: Option<Uuid>, at: SystemTime) -> Uuid {
    let mut tx = pool.begin().await.expect("test setup");
    let job = match org {
        Some(org) => NewJob::org(&FAIL, scope(org), &json!({})).expect("test setup"),
        None => NewJob::instance(&FAIL, &json!({})).expect("test setup"),
    };
    let Enqueued::Inserted(id) = enqueue(&mut tx, job, at).await.expect("test setup") else {
        panic!("a new row")
    };
    tx.commit().await.expect("test setup");
    let claimed = queue::claim(pool, &["default"], &[], 1, at)
        .await
        .expect("test setup")
        .claimed;
    queue::kill(pool, claimed[0].claim(), "the course is gone", at)
        .await
        .expect("test setup");
    id
}

async fn exists(pool: &PgPool, id: Uuid) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM jobs WHERE id = $1)")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("test setup")
}

// T6 (B8, B18, Q3): jobs.prune deletes dead jobs 30 days after `dead_at`, not 29, in every
// organization and the instance's own, and leaves living jobs.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn jobs_prune_deletes_dead_jobs_after_30_days(pool: PgPool) {
    let (a, b) = (new_id(), new_id());
    for (org, slug) in [(a, "org-a"), (b, "org-b")] {
        sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, $2, $2)")
            .bind(org)
            .bind(slug)
            .execute(&pool)
            .await
            .unwrap();
    }
    let dead = [
        dead_job(&pool, Some(a), t0()).await,
        dead_job(&pool, Some(b), t0()).await,
        dead_job(&pool, None, t0()).await,
    ];
    let younger = dead_job(&pool, Some(a), t0() + DAY).await;
    let mut tx = pool.begin().await.unwrap();
    let waiting = NewJob::org(&FAIL, scope(a), &json!({}))
        .unwrap()
        .run_at(t0() + 365 * DAY);
    let Enqueued::Inserted(waiting) = enqueue(&mut tx, waiting, t0()).await.unwrap() else {
        panic!("a new row")
    };
    tx.commit().await.unwrap();

    let registry = registry(10 * MINUTE);
    let clock = ManualClock::new(t0() + 29 * DAY);
    let worker = Worker::new(
        pool.clone(),
        registry.clone(),
        clock.clock(),
        Random::from_fn(|| 0),
        vec!["default"],
    );
    let prune_now = |at: SystemTime| {
        let (pool, registry) = (pool.clone(), registry.clone());
        async move {
            schedule::sync(&pool, &registry, at).await.unwrap();
            sqlx::query("UPDATE job_schedules SET next_run_at = timestamptz 'epoch'")
                .execute(&pool)
                .await
                .unwrap();
            schedule::tick(&pool, &registry, at).await.unwrap();
        }
    };
    prune_now(clock.clock().now()).await;
    worker.run_once(10).await.unwrap();
    for id in dead {
        assert!(exists(&pool, id).await, "29 days: kept");
    }

    clock.set(t0() + 30 * DAY);
    prune_now(clock.clock().now()).await;
    worker.run_once(10).await.unwrap();
    for id in dead {
        assert!(!exists(&pool, id).await, "30 days: pruned");
    }
    assert!(exists(&pool, younger).await, "died a day later: kept");
    assert!(exists(&pool, waiting).await, "a living job is never pruned");
}
