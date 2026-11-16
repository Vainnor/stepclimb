// SPDX-License-Identifier: AGPL-3.0-or-later
//! The job queue against a fresh database per test (docs/specs/m1/01-job-queue-and-outbox.md
//! B1 to B12, B25, B26; T1 to T10, T19). Workers run one pass at a time through
//! `Worker::run_once` on a manual clock. Needs `DATABASE_URL`.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, ScopeOrigin, Source,
    clock::ManualClock,
    id::{Uuid, new_id},
    users::{self, NewUser},
};
use stepclimb_jobs::{
    Enqueued, JobContext, Kind, Lease, NewJob, Outcome, Registry, Retry, Worker,
    backoff::Random,
    enqueue,
    kind::DEFAULT_LEASE,
    queue::{self, Error, MAX_PAYLOAD_BYTES},
};
use tokio::sync::Notify;
use tracing::Level;

const SECOND: Duration = Duration::from_secs(1);

/// Returns done.
const DONE: Kind = Kind::new("test.done", "default");
/// Returns retry with the message in its payload's `error`, or `attempt <n>`.
const RETRY: Kind = Kind::new("test.retry", "default");
/// Retries, with three attempts and a 1 s base.
const RETRY_THRICE: Kind = Kind::new("test.retry_thrice", "default").with_retry(Retry {
    base: SECOND,
    cap: Duration::from_secs(60),
    max_attempts: 3,
});
/// Returns fail.
const FAIL: Kind = Kind::new("test.fail", "default");
/// Panics.
const PANIC: Kind = Kind::new("test.panic", "default");
/// On its first attempt waits for `Shared::release`, then extends its lease and returns done.
const SLOW: Kind = Kind::new("test.slow", "default");
/// Extends its lease at once and returns done.
const EXTEND: Kind = Kind::new("test.extend", "default").with_lease(Duration::from_secs(30));
/// Records its scope and the number of users it can see through it, and returns done.
const COUNT_USERS: Kind = Kind::new("test.count_users", "default");
/// On another queue than the worker serves.
const OTHER_QUEUE: Kind = Kind::new("test.other_queue", "other");
/// Not in the registry: the job of a newer release.
const UNKNOWN: Kind = Kind::new("test.unknown", "default");

/// Every kind these tests enqueue for a handler to run. `registry_knows_every_enqueued_kind`
/// fails when one is missing from [`registry`].
const ENQUEUED: [&Kind; 9] = [
    &DONE,
    &RETRY,
    &RETRY_THRICE,
    &FAIL,
    &PANIC,
    &SLOW,
    &EXTEND,
    &COUNT_USERS,
    &OTHER_QUEUE,
];

/// What handlers saw, for the test to read.
#[derive(Clone, Default)]
struct Shared {
    started: Arc<Notify>,
    release: Arc<Notify>,
    leases: Arc<Mutex<Vec<Lease>>>,
    seen: Arc<Mutex<Vec<Seen>>>,
}

/// A scope a handler ran with, and the users it counted through it (-1 without a scope).
type Seen = (Option<OrgScope>, i64);

fn registry(shared: &Shared) -> Registry {
    let mut registry = Registry::new();
    registry
        .register(DONE, |_| async { Outcome::Done })
        .expect("test setup");
    registry.register(RETRY, retry_handler).expect("test setup");
    registry
        .register(RETRY_THRICE, retry_handler)
        .expect("test setup");
    registry
        .register(FAIL, |_| async {
            Outcome::fail("the payload names no course")
        })
        .expect("test setup");
    registry
        .register(PANIC, |_| async { panic!("a handler bug") })
        .expect("test setup");
    let s = shared.clone();
    registry
        .register(SLOW, move |ctx| slow_handler(ctx, s.clone()))
        .expect("test setup");
    let s = shared.clone();
    registry
        .register(EXTEND, move |ctx| extend_handler(ctx, s.clone()))
        .expect("test setup");
    let s = shared.clone();
    registry
        .register(COUNT_USERS, move |ctx| count_users_handler(ctx, s.clone()))
        .expect("test setup");
    registry
        .register(OTHER_QUEUE, |_| async { Outcome::Done })
        .expect("test setup");
    registry
}

async fn retry_handler(ctx: JobContext) -> Outcome {
    match ctx.payload().get("error").and_then(Value::as_str) {
        Some(error) => Outcome::retry(error),
        None => Outcome::retry(format!("attempt {}", ctx.job().attempt)),
    }
}

async fn slow_handler(ctx: JobContext, shared: Shared) -> Outcome {
    if ctx.job().attempt == 1 {
        shared.started.notify_one();
        shared.release.notified().await;
    }
    let lease = ctx.extend_lease().await.expect("extend the lease");
    shared.leases.lock().expect("test setup").push(lease);
    Outcome::Done
}

async fn extend_handler(ctx: JobContext, shared: Shared) -> Outcome {
    let lease = ctx.extend_lease().await.expect("extend the lease");
    shared.leases.lock().expect("test setup").push(lease);
    Outcome::Done
}

async fn count_users_handler(ctx: JobContext, shared: Shared) -> Outcome {
    let count = match ctx.scope() {
        Some(scope) => users::count_for_limit(ctx.pool(), scope)
            .await
            .expect("count users"),
        None => -1,
    };
    shared
        .seen
        .lock()
        .expect("test setup")
        .push((ctx.scope(), count));
    Outcome::Done
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

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the enqueuing request's credential scope"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

struct World {
    clock: ManualClock,
    shared: Shared,
    worker: Worker,
}

fn world(pool: PgPool, random: Random) -> World {
    let clock = ManualClock::new(t0());
    let shared = Shared::default();
    let worker = Worker::new(
        pool,
        registry(&shared),
        clock.clock(),
        random,
        vec!["default"],
    );
    World {
        clock,
        shared,
        worker,
    }
}

fn lowest_jitter() -> Random {
    Random::from_fn(|| 0)
}

async fn insert_org(pool: &PgPool, slug: &str) -> Uuid {
    let id = new_id();
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, $2, $2)")
        .bind(id)
        .bind(slug)
        .execute(pool)
        .await
        .expect("insert organization");
    id
}

async fn insert_user(pool: &PgPool, org: Uuid, name: &str) {
    let user = NewUser {
        display_name: name.into(),
        given_name: String::new(),
        family_name: String::new(),
        email: None,
        locale: None,
        tz: "UTC".into(),
        source: Source::Local,
        external_id: None,
    };
    users::create(pool, scope(org), &user, t0())
        .await
        .expect("create user");
}

/// Enqueue `job` in a transaction of its own, committed.
async fn put(pool: &PgPool, job: NewJob<'_>, now: SystemTime) -> Enqueued {
    let mut tx = pool.begin().await.expect("test setup");
    let enqueued = enqueue(&mut tx, job, now).await.expect("enqueue");
    tx.commit().await.expect("test setup");
    enqueued
}

async fn put_org(pool: &PgPool, kind: &Kind, org: Uuid, payload: Value) -> Uuid {
    let job = NewJob::org(kind, scope(org), &payload).expect("test setup");
    match put(pool, job, t0()).await {
        Enqueued::Inserted(id) => id,
        merged => panic!("expected a new row, got {merged:?}"),
    }
}

/// A job's row as the tests read it.
#[derive(Debug, PartialEq, Eq)]
struct Row {
    org_id: Option<Uuid>,
    queue: String,
    kind: String,
    attempts: i32,
    max_attempts: i32,
    run_at: i64,
    locked_until: Option<i64>,
    last_error: Option<String>,
    dead_at: Option<i64>,
}

/// The columns [`row`] selects, in order.
type RowColumns = (
    Option<Uuid>,
    String,
    String,
    i32,
    i32,
    i64,
    Option<i64>,
    Option<String>,
    Option<i64>,
);

async fn row(pool: &PgPool, id: Uuid) -> Option<Row> {
    let row: Option<RowColumns> = sqlx::query_as(
        "SELECT org_id, queue, kind, attempts, max_attempts,
                (extract(epoch FROM run_at) * 1000000)::bigint,
                (extract(epoch FROM locked_until) * 1000000)::bigint,
                last_error,
                (extract(epoch FROM dead_at) * 1000000)::bigint
         FROM jobs WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .expect("read job");
    row.map(|r| Row {
        org_id: r.0,
        queue: r.1,
        kind: r.2,
        attempts: r.3,
        max_attempts: r.4,
        run_at: r.5,
        locked_until: r.6,
        last_error: r.7,
        dead_at: r.8,
    })
}

async fn job_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM jobs")
        .fetch_one(pool)
        .await
        .expect("count jobs")
}

async fn claim_at(
    db: impl sqlx::PgExecutor<'_>,
    limit: u32,
    now: SystemTime,
) -> Vec<queue::Claimed> {
    queue::claim(db, &["default"], &[], limit, now)
        .await
        .expect("claim")
}

#[test]
fn registry_knows_every_enqueued_kind() {
    let registry = registry(&Shared::default());
    for kind in ENQUEUED {
        assert!(registry.knows(kind), "{} is not registered", kind.name);
    }
    assert!(!registry.knows(&UNKNOWN));
}

// T1 (B2).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_job_exists_exactly_when_its_transaction_commits(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let mut tx = pool.begin().await.unwrap();
    let job = NewJob::org(&DONE, scope(org), &json!({ "course": 1 })).unwrap();
    enqueue(&mut tx, job, t0()).await.unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(job_count(&pool).await, 0);

    let job = NewJob::org(&DONE, scope(org), &json!({ "course": 1 })).unwrap();
    let Enqueued::Inserted(id) = put(&pool, job, t0()).await else {
        panic!("a new row")
    };
    let stored = row(&pool, id).await.expect("the committed job");
    assert_eq!(
        stored,
        Row {
            org_id: Some(org),
            queue: "default".into(),
            kind: "test.done".into(),
            attempts: 0,
            max_attempts: 8,
            run_at: micros(t0()),
            locked_until: None,
            last_error: None,
            dead_at: None,
        }
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_instance_job_has_no_organization_and_runs_without_a_scope(pool: PgPool) {
    let w = world(pool.clone(), lowest_jitter());
    let job = NewJob::instance(&COUNT_USERS, &json!({})).unwrap();
    let Enqueued::Inserted(id) = put(&pool, job, t0()).await else {
        panic!("a new row")
    };
    assert_eq!(row(&pool, id).await.unwrap().org_id, None);
    assert_eq!(w.worker.run_once(10).await.unwrap(), 1);
    assert_eq!(*w.shared.seen.lock().unwrap(), [(None, -1)]);
    assert_eq!(row(&pool, id).await, None);
}

// B2.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_payload_over_64_kib_is_refused(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    // A JSON string serializes with its two quotes.
    let largest = "x".repeat(MAX_PAYLOAD_BYTES - 2);
    let job = NewJob::org(&DONE, scope(org), &largest).unwrap();
    put(&pool, job, t0()).await;
    let larger = "x".repeat(MAX_PAYLOAD_BYTES - 1);
    let refused = NewJob::org(&DONE, scope(org), &larger).unwrap_err();
    assert!(
        matches!(refused, Error::PayloadTooLarge(65_537)),
        "{refused:?}"
    );
}

// T2 (B3).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_waiting_job_absorbs_a_duplicate_and_a_claimed_one_does_not(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let keyed = |at: SystemTime| {
        NewJob::org(&DONE, scope(org), &json!({}))
            .unwrap()
            .dedupe("course-1")
            .unwrap()
            .run_at(at)
    };
    let Enqueued::Inserted(first) = put(&pool, keyed(t0() + 10 * SECOND), t0()).await else {
        panic!("a new row")
    };
    assert_eq!(
        put(&pool, keyed(t0() + 5 * SECOND), t0()).await,
        Enqueued::Merged(first)
    );
    assert_eq!(
        put(&pool, keyed(t0() + 20 * SECOND), t0()).await,
        Enqueued::Merged(first)
    );
    assert_eq!(job_count(&pool).await, 1);
    assert_eq!(
        row(&pool, first).await.unwrap().run_at,
        micros(t0() + 5 * SECOND)
    );

    let claimed = claim_at(&pool, 10, t0() + 5 * SECOND).await;
    assert_eq!(claimed.len(), 1);
    let Enqueued::Inserted(second) = put(&pool, keyed(t0() + 5 * SECOND), t0()).await else {
        panic!("a claimed job does not absorb a new one")
    };
    assert_ne!(first, second);
    assert_eq!(job_count(&pool).await, 2);
}

// B26: a dedupe key never reaches another organization's job.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_duplicate_of_another_organizations_job_is_refused(pool: PgPool) {
    let (a, b) = (
        insert_org(&pool, "org-a").await,
        insert_org(&pool, "org-b").await,
    );
    let later = t0() + 60 * SECOND;
    let of = |org: Option<Uuid>, at: SystemTime| {
        let job = match org {
            Some(org) => NewJob::org(&DONE, scope(org), &json!({})).unwrap(),
            None => NewJob::instance(&DONE, &json!({})).unwrap(),
        };
        job.dedupe("shared-key").unwrap().run_at(at)
    };
    let Enqueued::Inserted(first) = put(&pool, of(Some(a), later), t0()).await else {
        panic!("a new row")
    };
    for other in [Some(b), None] {
        let mut tx = pool.begin().await.unwrap();
        let refused = enqueue(&mut tx, of(other, t0()), t0()).await.unwrap_err();
        assert!(
            matches!(refused, Error::DedupeKeyOfAnotherOrganization),
            "{refused:?}"
        );
    }
    let kept = row(&pool, first).await.unwrap();
    assert_eq!((kept.org_id, kept.run_at), (Some(a), micros(later)));
    assert_eq!(job_count(&pool).await, 1);
}

#[test]
fn a_dedupe_key_must_be_1_to_200_characters() {
    let job = || NewJob::instance(&DONE, &json!({})).unwrap();
    let longest = "é".repeat(200);
    assert!(job().dedupe(&longest).is_ok());
    for bad in [String::new(), "é".repeat(201), "a\0b".to_owned()] {
        assert!(
            matches!(job().dedupe(&bad), Err(Error::DedupeKeyInvalid)),
            "{bad:?}"
        );
    }
}

// T3 (B4).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn two_claims_at_once_take_disjoint_jobs(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    for n in 0..10 {
        put_org(&pool, &DONE, org, json!({ "n": n })).await;
    }
    let mut first = pool.begin().await.unwrap();
    let mine = claim_at(&mut *first, 5, t0()).await;
    // The first claim's row locks are held until it commits; the second skips those rows
    // instead of waiting for them.
    let theirs = tokio::time::timeout(Duration::from_secs(10), claim_at(&pool, 10, t0()))
        .await
        .expect("the second claim skips locked rows instead of waiting");
    first.commit().await.unwrap();
    assert_eq!((mine.len(), theirs.len()), (5, 5));
    let mut ids: Vec<Uuid> = mine.iter().chain(&theirs).map(|c| c.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 10, "no job claimed twice");
    assert!(mine.iter().chain(&theirs).all(|c| c.attempts == 1));
    assert!(
        claim_at(&pool, 10, t0()).await.is_empty(),
        "every lease is live"
    );
}

// T3 (B4).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_job_is_claimed_once_its_run_at_has_come(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let job = NewJob::org(&DONE, scope(org), &json!({}))
        .unwrap()
        .run_at(t0() + SECOND);
    put(&pool, job, t0()).await;
    assert!(claim_at(&pool, 1, t0()).await.is_empty());
    assert!(
        claim_at(&pool, 1, t0() + SECOND - Duration::from_micros(1))
            .await
            .is_empty()
    );
    assert_eq!(claim_at(&pool, 1, t0() + SECOND).await.len(), 1);
}

// B4: in `run_at, id` order, up to the limit, of the worker's queues only.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_claim_takes_the_earliest_jobs_of_its_queues(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let at = |s: u32| {
        NewJob::org(&DONE, scope(org), &json!({}))
            .unwrap()
            .run_at(t0() + s * SECOND)
    };
    let Enqueued::Inserted(late) = put(&pool, at(3), t0()).await else {
        panic!()
    };
    let Enqueued::Inserted(early) = put(&pool, at(1), t0()).await else {
        panic!()
    };
    let Enqueued::Inserted(middle) = put(&pool, at(2), t0()).await else {
        panic!()
    };
    let other = NewJob::org(&OTHER_QUEUE, scope(org), &json!({})).unwrap();
    put(&pool, other, t0()).await;
    let now = t0() + 10 * SECOND;
    let mut first: Vec<Uuid> = claim_at(&pool, 2, now).await.iter().map(|c| c.id).collect();
    first.sort_by_key(|id| [early, middle, late].iter().position(|x| x == id));
    assert_eq!(first, [early, middle]);
    let rest: Vec<Uuid> = claim_at(&pool, 10, now)
        .await
        .iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(rest, [late], "the other queue's job is not claimed");
}

// B5: a live lease keeps the job; a lapsed one lets another claim take it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_lapsed_lease_lets_the_job_be_claimed_again(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let id = put_org(&pool, &DONE, org, json!({})).await;
    assert_eq!(claim_at(&pool, 1, t0()).await.len(), 1);
    let stored = row(&pool, id).await.unwrap();
    assert_eq!(stored.locked_until, Some(micros(t0() + DEFAULT_LEASE)));
    let before = t0() + DEFAULT_LEASE - Duration::from_micros(1);
    assert!(claim_at(&pool, 1, before).await.is_empty());
    let again = claim_at(&pool, 1, t0() + DEFAULT_LEASE).await;
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].attempts, 2);
}

// B5: a kind's own lease.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_kind_claims_with_its_own_lease_and_extends_it(pool: PgPool) {
    let w = world(pool.clone(), lowest_jitter());
    let org = insert_org(&pool, "org-a").await;
    let id = put_org(&pool, &EXTEND, org, json!({})).await;
    let claimed = queue::claim(&pool, &["default"], &[(EXTEND.name, EXTEND.lease)], 1, t0())
        .await
        .unwrap();
    assert_eq!(claimed.len(), 1);
    let stored = row(&pool, id).await.unwrap();
    assert_eq!(stored.locked_until, Some(micros(t0() + 30 * SECOND)));

    let later = t0() + 20 * SECOND;
    let lease = queue::extend_lease(&pool, claimed[0].claim(), EXTEND.lease, later)
        .await
        .unwrap();
    assert_eq!(lease, Lease::Held);
    let stored = row(&pool, id).await.unwrap();
    assert_eq!(stored.locked_until, Some(micros(later + 30 * SECOND)));

    // Through the worker: the handler's extension holds.
    w.clock.set(later + 30 * SECOND);
    assert_eq!(w.worker.run_once(1).await.unwrap(), 1);
    assert_eq!(*w.shared.leases.lock().unwrap(), [Lease::Held]);
    assert_eq!(row(&pool, id).await, None);
}

// T4 (B5, B6).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_handler_that_outlives_its_lease_changes_nothing(pool: PgPool) {
    let w = world(pool.clone(), lowest_jitter());
    let org = insert_org(&pool, "org-a").await;
    let id = put_org(&pool, &SLOW, org, json!({})).await;
    let worker = w.worker.clone();
    let first = tokio::spawn(async move { worker.run_once(1).await });
    w.shared.started.notified().await;

    w.clock.advance(DEFAULT_LEASE);
    let again = claim_at(&pool, 1, w.clock.clock().now()).await;
    assert_eq!(again.len(), 1, "the lapsed job is claimed again");
    assert_eq!(again[0].attempts, 2);

    w.shared.release.notify_one();
    assert_eq!(first.await.unwrap().unwrap(), 1);
    assert_eq!(*w.shared.leases.lock().unwrap(), [Lease::Lost]);
    let stored = row(&pool, id)
        .await
        .expect("the done-delete found the claim lost");
    assert_eq!(stored.attempts, 2);
    assert_eq!(stored.locked_until, Some(micros(t0() + 2 * DEFAULT_LEASE)));
}

// T5 (B6, B7).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_retried_job_waits_out_its_backoff(pool: PgPool) {
    let w = world(pool.clone(), lowest_jitter());
    let org = insert_org(&pool, "org-a").await;
    let id = put_org(&pool, &RETRY, org, json!({})).await;
    // With the lowest draw each delay is half of 5, 10 and 20 s; every draw lies between half
    // and the whole (the backoff unit tests cover the range).
    for (attempt, half_ms) in [(1, 2_500), (2, 5_000), (3, 10_000)] {
        let now = w.clock.clock().now();
        assert_eq!(w.worker.run_once(10).await.unwrap(), 1, "attempt {attempt}");
        let stored = row(&pool, id).await.unwrap();
        let delay = Duration::from_millis(half_ms);
        assert_eq!(stored.run_at, micros(now + delay), "attempt {attempt}");
        assert_eq!(stored.attempts, attempt);
        assert_eq!(stored.locked_until, None);
        assert_eq!(
            stored.last_error.as_deref(),
            Some(format!("attempt {attempt}").as_str())
        );
        w.clock.advance(delay - Duration::from_micros(1));
        assert_eq!(
            w.worker.run_once(10).await.unwrap(),
            0,
            "not before its run_at"
        );
        w.clock.advance(Duration::from_micros(1));
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_highest_draw_waits_the_whole_delay(pool: PgPool) {
    // 2,500,000 is the top of the first retry's range: 5 s, 2.5 s plus 2,500,001 values.
    let w = world(pool.clone(), Random::from_fn(|| 2_500_000));
    let org = insert_org(&pool, "org-a").await;
    let id = put_org(&pool, &RETRY, org, json!({})).await;
    w.worker.run_once(1).await.unwrap();
    assert_eq!(
        row(&pool, id).await.unwrap().run_at,
        micros(t0() + 5 * SECOND)
    );
}

// T6 (B6, B8), up to pruning (issue I3).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_failed_job_is_dead_at_once_and_never_claimed_again(pool: PgPool) {
    let w = world(pool.clone(), lowest_jitter());
    let org = insert_org(&pool, "org-a").await;
    let id = put_org(&pool, &FAIL, org, json!({})).await;
    assert_eq!(w.worker.run_once(10).await.unwrap(), 1);
    let stored = row(&pool, id).await.expect("a dead job keeps its row");
    assert_eq!(stored.dead_at, Some(micros(t0())));
    assert_eq!(stored.attempts, 1);
    assert_eq!(stored.locked_until, None);
    assert_eq!(
        stored.last_error.as_deref(),
        Some("the payload names no course")
    );
    w.clock.advance(Duration::from_secs(365 * 24 * 60 * 60));
    assert_eq!(w.worker.run_once(10).await.unwrap(), 0);
}

// T6 (B6, B8).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_job_that_uses_its_attempts_is_dead(pool: PgPool) {
    let w = world(pool.clone(), lowest_jitter());
    let org = insert_org(&pool, "org-a").await;
    let id = put_org(&pool, &RETRY_THRICE, org, json!({})).await;
    for attempt in 1..=3 {
        assert_eq!(w.worker.run_once(10).await.unwrap(), 1, "attempt {attempt}");
        let stored = row(&pool, id).await.unwrap();
        assert_eq!(stored.dead_at.is_some(), attempt == 3, "attempt {attempt}");
        w.clock.advance(Duration::from_secs(60 * 60));
    }
    let stored = row(&pool, id).await.unwrap();
    assert_eq!(stored.max_attempts, 3);
    assert_eq!(stored.last_error.as_deref(), Some("attempt 3"));
    w.clock.advance(Duration::from_secs(365 * 24 * 60 * 60));
    assert_eq!(w.worker.run_once(10).await.unwrap(), 0);
}

// T7 (B6).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_panicking_handler_is_retried_and_the_worker_goes_on(pool: PgPool) {
    let w = world(pool.clone(), lowest_jitter());
    let org = insert_org(&pool, "org-a").await;
    let panics = put_org(&pool, &PANIC, org, json!({})).await;
    let done = put_org(&pool, &DONE, org, json!({})).await;
    assert_eq!(w.worker.run_once(10).await.unwrap(), 2);
    let stored = row(&pool, panics).await.unwrap();
    assert_eq!((stored.attempts, stored.dead_at), (1, None));
    assert_eq!(
        stored.run_at,
        micros(t0() + 2_500 * Duration::from_millis(1))
    );
    assert_eq!(
        stored.last_error.as_deref(),
        Some("the job's handler panicked")
    );
    assert_eq!(row(&pool, done).await, None);

    let next = put_org(&pool, &DONE, org, json!({})).await;
    assert_eq!(w.worker.run_once(10).await.unwrap(), 1);
    assert_eq!(row(&pool, next).await, None);
}

// T8 (B9).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_long_error_is_stored_cut_at_a_character_boundary(pool: PgPool) {
    let w = world(pool.clone(), lowest_jitter());
    let org = insert_org(&pool, "org-a").await;
    // 2,047 bytes, then a 3-byte character across the limit, then the rest of 10 KiB.
    let error = format!("{}€{}", "a".repeat(2047), "b".repeat(10 * 1024 - 2050));
    let id = put_org(&pool, &RETRY, org, json!({ "error": error })).await;
    w.worker.run_once(1).await.unwrap();
    let stored = row(&pool, id).await.unwrap().last_error.unwrap();
    assert_eq!(stored, "a".repeat(2047));
}

// T9 (B10).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_organizations_job_runs_with_that_organizations_scope(pool: PgPool) {
    let w = world(pool.clone(), lowest_jitter());
    let (a, b) = (
        insert_org(&pool, "org-a").await,
        insert_org(&pool, "org-b").await,
    );
    insert_user(&pool, a, "Avery Quinn").await;
    for name in ["Blake Ito", "Casey Moreau", "Drew Haddad"] {
        insert_user(&pool, b, name).await;
    }
    put_org(&pool, &COUNT_USERS, a, json!({})).await;
    w.worker.run_once(1).await.unwrap();
    let seen = w.shared.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    let (scope, count) = seen[0];
    let scope = scope.expect("an organization's job has a scope");
    assert_eq!(
        (scope.org_id(), scope.origin(), count),
        (a, ScopeOrigin::Instance, 1)
    );
}

// T9 (B10). The foreign key keeps an organization with jobs from being deleted today; the test
// drops it to reach the check that guards a job whose organization is gone.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_job_whose_organization_is_gone_fails(pool: PgPool) {
    let w = world(pool.clone(), lowest_jitter());
    let org = insert_org(&pool, "org-gone").await;
    let id = put_org(&pool, &COUNT_USERS, org, json!({})).await;
    sqlx::query("ALTER TABLE jobs DROP CONSTRAINT jobs_org_id_fkey")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(org)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(w.worker.run_once(1).await.unwrap(), 1);
    assert!(
        w.shared.seen.lock().unwrap().is_empty(),
        "the handler never ran"
    );
    let stored = row(&pool, id).await.unwrap();
    assert_eq!(stored.dead_at, Some(micros(t0())));
    assert_eq!(
        stored.last_error.as_deref(),
        Some("the job's organization does not exist")
    );
}

// T10 (B11).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_job_of_an_unknown_kind_is_released_without_counting(pool: PgPool) {
    let w = world(pool.clone(), lowest_jitter());
    let org = insert_org(&pool, "org-a").await;
    let id = put_org(&pool, &UNKNOWN, org, json!({})).await;
    assert_eq!(w.worker.run_once(1).await.unwrap(), 1);
    let stored = row(&pool, id).await.unwrap();
    assert_eq!(stored.attempts, 0);
    assert_eq!(stored.locked_until, None);
    assert_eq!(stored.dead_at, None);
    assert_eq!(stored.run_at, micros(t0() + 60 * SECOND));
}

// Data: a retried job that would collide with a waiting duplicate is deleted.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_retry_covered_by_a_waiting_duplicate_is_deleted(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let keyed = || {
        NewJob::org(&RETRY, scope(org), &json!({}))
            .unwrap()
            .dedupe("course-1")
            .unwrap()
    };
    let Enqueued::Inserted(running) = put(&pool, keyed(), t0()).await else {
        panic!()
    };
    let claimed = claim_at(&pool, 1, t0()).await;
    let Enqueued::Inserted(waiting) = put(&pool, keyed(), t0()).await else {
        panic!()
    };
    let released = queue::retry(&pool, claimed[0].claim(), t0(), "busy", t0())
        .await
        .unwrap();
    assert_eq!(released, queue::Released::Covered);
    assert_eq!(row(&pool, running).await, None);
    assert!(row(&pool, waiting).await.is_some());
}

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("test setup").extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// T19 (B25).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_job_run_logs_its_identity_and_outcome_and_never_its_payload(pool: PgPool) {
    let w = world(pool.clone(), lowest_jitter());
    let org = insert_org(&pool, "org-a").await;
    let marker = "payload-marker-5d1c";
    let id = put_org(&pool, &DONE, org, json!({ "note": marker })).await;
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    w.worker.run_once(1).await.unwrap();
    drop(guard);
    let log = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    let started = log.lines().find(|l| l.contains("job started")).expect(&log);
    let ended = log.lines().find(|l| l.contains("job ended")).expect(&log);
    for line in [started, ended] {
        assert!(line.contains("test.done"), "{line}");
        assert!(line.contains(&id.to_string()), "{line}");
        assert!(line.contains(&org.to_string()), "{line}");
        assert!(line.contains("attempt=1"), "{line}");
    }
    assert!(started.contains("DEBUG"), "{started}");
    assert!(ended.contains("INFO") && ended.contains("done"), "{ended}");
    assert!(!log.contains(marker), "{log}");
}
