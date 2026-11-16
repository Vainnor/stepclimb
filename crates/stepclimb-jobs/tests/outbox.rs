// SPDX-License-Identifier: AGPL-3.0-or-later
//! The transactional outbox against a fresh database per test
//! (docs/specs/m1/01-job-queue-and-outbox.md B19 to B24; T15 to T18). Dispatch passes run one at
//! a time on a manual clock; the dispatcher loop runs with a shortened poll. Needs
//! `DATABASE_URL`.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope,
    clock::ManualClock,
    id::{Uuid, new_id},
};
use stepclimb_jobs::{
    Kind, Outcome, Registry, Worker,
    backoff::Random,
    outbox::{self, Dispatched, Error, Topic, UnknownTopic, dispatch, dispatch_in},
    prune, schedule,
};
use tokio::sync::{Notify, oneshot};
use tracing::Level;

const HOUR: Duration = Duration::from_secs(60 * 60);
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Two subscribers.
const HAPPENED: Topic = Topic::new("test.happened", 1);
/// None.
const IGNORED: Topic = Topic::new("test.ignored", 1);
/// Not registered: a newer release's topic.
const NEWER: Topic = Topic::new("test.newer", 1);
/// Not registered: a newer release's version of a registered topic.
const HAPPENED_V2: Topic = Topic::new("test.happened", 2);

/// Records the event it read.
const FIRST: Kind = Kind::new("test.first", "default");
/// Records the event it read.
const SECOND: Kind = Kind::new("test.second", "default");

/// What the subscribers read, as `(kind, event)`.
type Seen = Arc<Mutex<Vec<(&'static str, outbox::Event)>>>;

fn registry(seen: &Seen) -> Registry {
    let mut registry = Registry::new();
    registry.register_topic(HAPPENED).expect("test setup");
    registry.register_topic(IGNORED).expect("test setup");
    for kind in [FIRST, SECOND] {
        let seen = Arc::clone(seen);
        registry
            .subscribe(&HAPPENED, kind, move |ctx| {
                let seen = Arc::clone(&seen);
                async move {
                    match outbox::event(&ctx).await {
                        Ok(event) => {
                            seen.lock()
                                .expect("test setup")
                                .push((ctx.job().kind, event));
                            Outcome::Done
                        }
                        Err(outcome) => outcome,
                    }
                }
            })
            .expect("test setup");
    }
    prune::register(&mut registry).expect("test setup");
    registry
}

/// 2025-11-13T02:13:20.123456Z.
fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the emitting request's credential scope"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
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

/// Emit an event of `topic` for `org` with `data` at `at`, committed.
async fn emit_at(pool: &PgPool, org: Uuid, topic: &Topic, data: Value, at: SystemTime) -> Uuid {
    let mut tx = pool.begin().await.expect("test setup");
    let id = outbox::emit(&mut tx, scope(org), topic, &data, at)
        .await
        .expect("test setup");
    tx.commit().await.expect("test setup");
    id
}

async fn emit(pool: &PgPool, org: Uuid, topic: &Topic) -> Uuid {
    emit_at(pool, org, topic, json!({ "n": 1 }), t0()).await
}

/// Every job as `(kind, org_id, payload)`, by kind and payload.
async fn jobs(pool: &PgPool) -> Vec<(String, Option<Uuid>, Value)> {
    sqlx::query_as("SELECT kind, org_id, payload FROM jobs ORDER BY kind, payload::text")
        .fetch_all(pool)
        .await
        .expect("test setup")
}

async fn undispatched(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM outbox WHERE dispatched_at IS NULL")
        .fetch_one(pool)
        .await
        .expect("test setup")
}

async fn exists(pool: &PgPool, id: Uuid) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM outbox WHERE id = $1)")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("test setup")
}

// T15 (B19): an event exists exactly when its transaction commits.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_event_exists_only_when_its_transaction_commits(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let mut tx = pool.begin().await.expect("test setup");
    let rolled_back = outbox::emit(&mut tx, scope(org), &HAPPENED, &json!({}), t0())
        .await
        .expect("test setup");
    tx.rollback().await.expect("test setup");
    assert!(!exists(&pool, rolled_back).await);
    assert_eq!(
        outbox::get(&pool, scope(org), rolled_back)
            .await
            .expect("test setup"),
        None
    );

    let committed = emit(&pool, org, &HAPPENED).await;
    let event = outbox::get(&pool, scope(org), committed)
        .await
        .expect("test setup")
        .expect("committed");
    assert_eq!((event.org_id, event.topic.as_str()), (org, "test.happened"));
    assert!(!event.dispatched);
}

// T17 (B20): the envelope has the topic as `type`, the clock's time in RFC 3339 UTC and the
// data; the event id is the row id.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_stored_payload_is_the_event_envelope(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let data = json!({ "item_id": "0190f0e2-0000-7000-8000-000000000001", "score": 3 });
    let id = emit_at(&pool, org, &HAPPENED, data.clone(), t0()).await;
    let event = outbox::get(&pool, scope(org), id)
        .await
        .expect("test setup")
        .expect("test setup");
    assert_eq!(event.id, id);
    assert_eq!(event.version, 1);
    assert_eq!(
        event.payload,
        json!({
            "type": "test.happened",
            "timestamp": "2025-11-13T02:13:20.123456Z",
            "data": data,
        })
    );
    let midnight = UNIX_EPOCH + Duration::from_secs(1_767_225_600);
    let id = emit_at(&pool, org, &HAPPENED, json!({}), midnight).await;
    let event = outbox::get(&pool, scope(org), id)
        .await
        .expect("test setup")
        .expect("test setup");
    assert_eq!(event.payload["timestamp"], "2026-01-01T00:00:00.000000Z");
}

// B20: `data` is a JSON object and the topic a valid one; anything else is refused before it
// reaches the database.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn emit_refuses_data_that_is_not_an_object_and_bad_topics(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let mut tx = pool.begin().await.expect("test setup");
    for data in [json!([1]), json!("x"), json!(null)] {
        let refused = outbox::emit(&mut tx, scope(org), &HAPPENED, &data, t0()).await;
        assert!(matches!(refused, Err(Error::DataNotObject)), "{data}");
    }
    let bad = Topic::new("happened", 1);
    let refused = outbox::emit(&mut tx, scope(org), &bad, &json!({}), t0()).await;
    assert!(matches!(refused, Err(Error::Topic(_))));
    tx.commit().await.expect("test setup");
    assert_eq!(undispatched(&pool).await, 0);
}

// T16 (B21): one job per event and subscriber, with `{ "event_id" }` and the event's
// organization; every event dispatched; a second pass enqueues nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_pass_enqueues_one_job_per_event_and_subscriber(pool: PgPool) {
    let (a, b) = (
        insert_org(&pool, "org-a").await,
        insert_org(&pool, "org-b").await,
    );
    let happened = [
        (emit(&pool, a, &HAPPENED).await, a),
        (emit(&pool, b, &HAPPENED).await, b),
        (emit(&pool, a, &HAPPENED).await, a),
    ];
    let ignored = emit(&pool, b, &IGNORED).await;
    let registry = registry(&Seen::default());

    let done = dispatch(&pool, &registry, t0()).await.expect("test setup");
    assert_eq!(done, Dispatched { events: 4, jobs: 6 });
    let mut expected: Vec<(String, Option<Uuid>, Value)> = happened
        .iter()
        .flat_map(|&(id, org)| {
            ["test.first", "test.second"]
                .map(|kind| (kind.to_owned(), Some(org), json!({ "event_id": id })))
        })
        .collect();
    expected.sort_by_key(|(kind, _, payload)| (kind.clone(), payload.to_string()));
    assert_eq!(jobs(&pool).await, expected);
    assert_eq!(undispatched(&pool).await, 0);
    let ignored = outbox::get(&pool, scope(b), ignored)
        .await
        .expect("test setup")
        .expect("test setup");
    assert!(ignored.dispatched, "no subscriber: dispatched with no job");

    assert_eq!(
        dispatch(&pool, &registry, t0()).await.expect("test setup"),
        Dispatched::default()
    );
    assert_eq!(jobs(&pool).await.len(), 6);
}

// B21: an event of a topic or version this binary does not know waits for a release that
// knows it, and is counted as such.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_event_of_an_unknown_topic_or_version_stays_undispatched(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let newer = emit(&pool, org, &NEWER).await;
    let v2 = emit(&pool, org, &HAPPENED_V2).await;
    emit(&pool, org, &HAPPENED_V2).await;
    let registry = registry(&Seen::default());
    assert_eq!(
        dispatch(&pool, &registry, t0()).await.expect("test setup"),
        Dispatched::default()
    );
    for id in [newer, v2] {
        let event = outbox::get(&pool, scope(org), id)
            .await
            .expect("test setup")
            .expect("test setup");
        assert!(!event.dispatched, "{}", event.topic);
    }
    assert_eq!(
        outbox::get(&pool, scope(org), v2)
            .await
            .expect("test setup")
            .expect("test setup")
            .version,
        2
    );
    assert!(
        jobs(&pool).await.is_empty(),
        "no v1 subscriber got a v2 event"
    );
    let unknown = |topic: &str, version, events| UnknownTopic {
        topic: topic.to_owned(),
        version,
        events,
    };
    assert_eq!(
        outbox::unknown_topics(&pool, &registry)
            .await
            .expect("test setup"),
        [unknown("test.happened", 2, 2), unknown("test.newer", 1, 1)]
    );

    let mut newer_registry = registry.clone();
    newer_registry.register_topic(NEWER).expect("test setup");
    assert_eq!(
        dispatch(&pool, &newer_registry, t0())
            .await
            .expect("test setup"),
        Dispatched { events: 1, jobs: 0 }
    );
    assert_eq!(undispatched(&pool).await, 2, "the v2 events still wait");
    assert_eq!(
        outbox::unknown_topics(&pool, &newer_registry)
            .await
            .expect("test setup"),
        [unknown("test.happened", 2, 2)]
    );
}

// Data: the column refuses a version below 1.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_outbox_refuses_a_version_below_1(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    for version in [0, -1] {
        let refused = sqlx::query(
            "INSERT INTO outbox (id, org_id, topic, version, payload)
             VALUES (gen_random_uuid(), $1, 'test.happened', $2, '{}')",
        )
        .bind(org)
        .bind(version)
        .execute(&pool)
        .await;
        assert!(refused.is_err(), "{version}");
    }
}

// T16 (B21): while one pass holds its events, a second pass on another connection skips them
// instead of waiting, and once the first commits nothing is left for it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_second_dispatcher_skips_the_events_a_first_holds(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    for _ in 0..5 {
        emit(&pool, org, &HAPPENED).await;
    }
    let registry = registry(&Seen::default());
    let mut first = pool.begin().await.expect("test setup");
    let mine = dispatch_in(&mut first, &registry, 3, t0())
        .await
        .expect("test setup");
    let theirs = tokio::time::timeout(Duration::from_secs(10), dispatch(&pool, &registry, t0()))
        .await
        .expect("the second pass skips locked events instead of waiting")
        .expect("test setup");
    assert_eq!(mine, Dispatched { events: 3, jobs: 6 });
    assert_eq!(theirs, Dispatched { events: 2, jobs: 4 });
    first.commit().await.expect("test setup");
    assert_eq!(
        dispatch(&pool, &registry, t0()).await.expect("test setup"),
        Dispatched::default()
    );
    assert_one_job_per_event_and_subscriber(&pool, 5).await;
}

// T16 (B21): two dispatchers racing over the same backlog enqueue each job once.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn two_dispatchers_at_once_enqueue_each_job_once(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    sqlx::query(
        "INSERT INTO outbox (id, org_id, topic, version, payload)
         SELECT gen_random_uuid(), $1, 'test.happened', 1, '{}' FROM generate_series(1, 450)",
    )
    .bind(org)
    .execute(&pool)
    .await
    .expect("test setup");
    let registry = registry(&Seen::default());
    let drain = || {
        let (pool, registry) = (pool.clone(), registry.clone());
        async move {
            let mut events = 0;
            loop {
                let done = dispatch(&pool, &registry, t0()).await.expect("test setup");
                if done.events == 0 {
                    return events;
                }
                events += done.events;
            }
        }
    };
    let (a, b) = tokio::join!(tokio::spawn(drain()), tokio::spawn(drain()));
    assert_eq!(a.expect("test setup") + b.expect("test setup"), 450);
    assert_eq!(undispatched(&pool).await, 0);
    assert_one_job_per_event_and_subscriber(&pool, 450).await;
}

/// Each of the `events` events has exactly one job of each subscriber.
async fn assert_one_job_per_event_and_subscriber(pool: &PgPool, events: i64) {
    let (total, distinct): (i64, i64) =
        sqlx::query_as("SELECT count(*), count(DISTINCT (kind, payload->>'event_id')) FROM jobs")
            .fetch_one(pool)
            .await
            .expect("test setup");
    assert_eq!((total, distinct), (2 * events, 2 * events));
    let missing: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox o, (VALUES ('test.first'), ('test.second')) k (kind)
         WHERE NOT EXISTS (
             SELECT 1 FROM jobs j
             WHERE j.kind = k.kind AND j.payload->>'event_id' = o.id::text
               AND j.org_id = o.org_id)",
    )
    .fetch_one(pool)
    .await
    .expect("test setup");
    assert_eq!(missing, 0);
}

// B21, B12: a pass that does not commit leaves no job and every event undispatched, so the
// next pass dispatches them: an event reaches its subscribers at least once.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_pass_that_does_not_commit_leaves_its_events_for_the_next(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    emit(&pool, org, &HAPPENED).await;
    emit(&pool, org, &HAPPENED).await;
    let registry = registry(&Seen::default());
    let mut cut_off = pool.begin().await.expect("test setup");
    let done = dispatch_in(&mut cut_off, &registry, 100, t0())
        .await
        .expect("test setup");
    assert_eq!(done, Dispatched { events: 2, jobs: 4 });
    cut_off.rollback().await.expect("test setup");
    assert!(jobs(&pool).await.is_empty());
    assert_eq!(undispatched(&pool).await, 2);

    let done = dispatch(&pool, &registry, t0()).await.expect("test setup");
    assert_eq!(done, Dispatched { events: 2, jobs: 4 });
    assert_one_job_per_event_and_subscriber(&pool, 2).await;
}

/// Reads its event, then on its first attempt waits for the test before it returns done.
const SLOW_READER: Kind = Kind::new("test.slow_reader", "default");

// B21, B12: a subscriber's job that runs a second time, after its first run's lease lapsed,
// reads the same event with the event's organization both times.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_subscriber_that_runs_twice_reads_the_same_event_twice(pool: PgPool) {
    let (a, b) = (
        insert_org(&pool, "org-a").await,
        insert_org(&pool, "org-b").await,
    );
    let id = emit_at(&pool, a, &HAPPENED, json!({ "n": 7 }), t0()).await;
    emit(&pool, b, &IGNORED).await;
    let seen = Seen::default();
    let (started, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let mut registry = Registry::new();
    registry.register_topic(HAPPENED).expect("test setup");
    registry.register_topic(IGNORED).expect("test setup");
    let (reads, first_started, first_release) = (
        Arc::clone(&seen),
        Arc::clone(&started),
        Arc::clone(&release),
    );
    registry
        .subscribe(&HAPPENED, SLOW_READER, move |ctx| {
            let (reads, started, release) = (
                Arc::clone(&reads),
                Arc::clone(&first_started),
                Arc::clone(&first_release),
            );
            async move {
                let event = match outbox::event(&ctx).await {
                    Ok(event) => event,
                    Err(outcome) => return outcome,
                };
                reads
                    .lock()
                    .expect("test setup")
                    .push((ctx.job().kind, event));
                if ctx.job().attempt == 1 {
                    started.notify_one();
                    release.notified().await;
                }
                Outcome::Done
            }
        })
        .expect("test setup");
    dispatch(&pool, &registry, t0()).await.expect("test setup");
    let clock = ManualClock::new(t0());
    let worker = Worker::new(
        pool.clone(),
        registry,
        clock.clock(),
        Random::from_fn(|| 0),
        vec!["default"],
    );

    let first = tokio::spawn({
        let worker = worker.clone();
        async move { worker.run_once(1).await }
    });
    started.notified().await;
    clock.set(t0() + Duration::from_secs(301));
    assert_eq!(
        worker.run_once(1).await.expect("test setup"),
        1,
        "claimed again"
    );
    release.notify_one();
    assert_eq!(first.await.expect("test setup").expect("test setup"), 1);

    let reads = seen.lock().expect("test setup").clone();
    assert_eq!(reads.len(), 2, "the handler ran twice");
    assert_eq!(reads[0], reads[1], "the same event both times");
    assert_eq!((reads[0].1.id, reads[0].1.org_id), (id, a));
    assert_eq!(reads[0].1.payload["data"], json!({ "n": 7 }));
    assert!(
        jobs(&pool).await.is_empty(),
        "the second run's done deleted the job"
    );
}

// B26: an event is read only with its own organization's scope.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_event_is_not_read_with_another_organizations_scope(pool: PgPool) {
    let (a, b) = (
        insert_org(&pool, "org-a").await,
        insert_org(&pool, "org-b").await,
    );
    let id = emit(&pool, a, &HAPPENED).await;
    assert_eq!(
        outbox::get(&pool, scope(b), id).await.expect("test setup"),
        None
    );
    assert!(
        outbox::get(&pool, scope(a), id)
            .await
            .expect("test setup")
            .is_some()
    );
}

// B6, B21: a subscriber's job whose event no longer exists fails at once.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_job_whose_event_is_gone_fails(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let id = emit(&pool, org, &HAPPENED).await;
    let seen = Seen::default();
    let registry = registry(&seen);
    dispatch(&pool, &registry, t0()).await.expect("test setup");
    sqlx::query("DELETE FROM outbox WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("test setup");
    let clock = ManualClock::new(t0());
    let worker = Worker::new(
        pool.clone(),
        registry,
        clock.clock(),
        Random::from_fn(|| 0),
        vec!["default"],
    );
    worker.run_once(10).await.expect("test setup");
    assert!(seen.lock().expect("test setup").is_empty());
    let dead: Vec<(Option<String>,)> =
        sqlx::query_as("SELECT last_error FROM jobs WHERE dead_at IS NOT NULL")
            .fetch_all(&pool)
            .await
            .expect("test setup");
    assert_eq!(
        dead,
        vec![(Some("the job's event does not exist".to_owned()),); 2]
    );
}

// B21, Q2: the loop runs again at once after a full batch, so a backlog of several batches is
// dispatched without waiting for the poll interval, and it stops on shutdown.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_loop_runs_again_at_once_after_a_full_batch(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    sqlx::query(
        "INSERT INTO outbox (id, org_id, topic, version, payload)
         SELECT gen_random_uuid(), $1, 'test.ignored', 1, '{}' FROM generate_series(1, 250)",
    )
    .bind(org)
    .execute(&pool)
    .await
    .expect("test setup");
    let clock = ManualClock::new(t0());
    let (stop, stopped) = oneshot::channel::<()>();
    let dispatcher = tokio::spawn(outbox::dispatch::run(
        pool.clone(),
        registry(&Seen::default()),
        clock.clock(),
        Duration::from_secs(3600),
        async {
            let _ = stopped.await;
        },
    ));
    let drained = tokio::time::timeout(Duration::from_secs(20), async {
        while undispatched(&pool).await > 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(
        drained.is_ok(),
        "250 events in three passes, without a poll"
    );
    stop.send(()).expect("test setup");
    tokio::time::timeout(Duration::from_secs(10), dispatcher)
        .await
        .expect("the loop stops on shutdown")
        .expect("test setup");
}

// T18 (B23, Q3): dispatched events are pruned 30 days after dispatch, not 29; an undispatched
// event is never pruned, however old.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn jobs_prune_deletes_events_30_days_after_dispatch(pool: PgPool) {
    let (a, b) = (
        insert_org(&pool, "org-a").await,
        insert_org(&pool, "org-b").await,
    );
    let registry = registry(&Seen::default());
    let old = [
        emit(&pool, a, &IGNORED).await,
        emit(&pool, b, &IGNORED).await,
    ];
    dispatch(&pool, &registry, t0()).await.expect("test setup");
    let younger = emit(&pool, a, &IGNORED).await;
    dispatch(&pool, &registry, t0() + DAY)
        .await
        .expect("test setup");
    let waiting = emit(&pool, a, &NEWER).await;

    let clock = ManualClock::new(t0());
    let worker = Worker::new(
        pool.clone(),
        registry.clone(),
        clock.clock(),
        Random::from_fn(|| 0),
        vec!["default"],
    );
    let prune_at = |at: SystemTime| {
        let (pool, registry, worker, clock) = (
            pool.clone(),
            registry.clone(),
            worker.clone(),
            clock.clone(),
        );
        async move {
            clock.set(at);
            schedule::sync(&pool, &registry, at)
                .await
                .expect("test setup");
            sqlx::query("UPDATE job_schedules SET next_run_at = timestamptz 'epoch'")
                .execute(&pool)
                .await
                .expect("test setup");
            schedule::tick(&pool, &registry, at)
                .await
                .expect("test setup");
            worker.run_once(10).await.expect("test setup");
        }
    };
    prune_at(t0() + 29 * DAY).await;
    for id in old {
        assert!(exists(&pool, id).await, "29 days: kept");
    }
    prune_at(t0() + 30 * DAY).await;
    for id in old {
        assert!(!exists(&pool, id).await, "30 days: pruned");
    }
    assert!(exists(&pool, younger).await, "dispatched a day later: kept");
    prune_at(t0() + 40 * DAY).await;
    assert!(!exists(&pool, younger).await);
    assert!(exists(&pool, waiting).await, "undispatched: never pruned");
}

/// Run `jobs.prune` once at `at` with `registry`, and return every log line it wrote.
async fn prune_logged(pool: &PgPool, registry: &Registry, at: SystemTime) -> String {
    schedule::sync(pool, registry, at)
        .await
        .expect("test setup");
    schedule::tick(pool, registry, at)
        .await
        .expect("test setup");
    let clock = ManualClock::new(at);
    let worker = Worker::new(
        pool.clone(),
        registry.clone(),
        clock.clock(),
        Random::from_fn(|| 0),
        vec!["default"],
    );
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    worker.run_once(10).await.expect("test setup");
    drop(guard);
    let bytes = captured.0.lock().expect("the log buffer").clone();
    String::from_utf8(bytes).expect("UTF-8 logs")
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

// B23: a backlog of dispatched events larger than one delete batch is pruned in full.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn jobs_prune_clears_an_event_backlog_larger_than_a_batch(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    sqlx::query(
        "INSERT INTO outbox (id, org_id, topic, version, payload, dispatched_at)
         SELECT gen_random_uuid(), $1, 'test.ignored', 1, '{}',
                timestamptz 'epoch' + $2::bigint * interval '1 microsecond'
         FROM generate_series(1, $3)",
    )
    .bind(org)
    .bind(
        i64::try_from(
            t0().duration_since(UNIX_EPOCH)
                .expect("test setup")
                .as_micros(),
        )
        .expect("test setup"),
    )
    .bind(i64::from(prune::BATCH) + 1)
    .execute(&pool)
    .await
    .expect("test setup");
    prune_logged(&pool, &registry(&Seen::default()), t0() + 30 * DAY).await;
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox")
        .fetch_one(&pool)
        .await
        .expect("test setup");
    assert_eq!(
        left,
        0,
        "{} events, in batches of {}",
        prune::BATCH + 1,
        prune::BATCH
    );
}

// B21: `jobs.prune` logs at `warn` the undispatched events of each topic and version the
// registry does not know, with their count, and nothing when there are none.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn jobs_prune_warns_of_events_of_unknown_topics(pool: PgPool) {
    let org = insert_org(&pool, "org-a").await;
    let registry = registry(&Seen::default());
    let quiet = prune_logged(&pool, &registry, t0()).await;
    assert!(!quiet.contains("does not know"), "{quiet}");
    emit(&pool, org, &NEWER).await;
    emit(&pool, org, &NEWER).await;
    emit(&pool, org, &HAPPENED).await;
    let log = prune_logged(&pool, &registry, t0() + 2 * HOUR).await;
    let line = log
        .lines()
        .find(|l| l.contains("does not know"))
        .expect(&log);
    assert!(line.contains("WARN"), "{line}");
    assert!(line.contains("topic=\"test.newer\""), "{line}");
    assert!(line.contains("version=1"), "{line}");
    assert!(line.contains("events=2"), "{line}");
    assert_eq!(log.matches("does not know").count(), 1, "{log}");
}

// B24: emitting code's tests check that the registry knows each topic constant it emits.
#[test]
fn the_registry_knows_every_emitted_topic() {
    let registry = registry(&Seen::default());
    for topic in [&HAPPENED, &IGNORED] {
        assert!(registry.knows_topic(topic), "{}", topic.name);
    }
    assert!(!registry.knows_topic(&NEWER));
    assert!(!registry.knows_topic(&HAPPENED_V2));
}
