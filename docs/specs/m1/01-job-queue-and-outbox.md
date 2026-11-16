# Job queue and outbox

- **Status:** Draft
- **Milestone:** M1
- **Plan:** `docs/plan/technical.md` -> Architecture and stack (Jobs: Postgres queue with `FOR
  UPDATE SKIP LOCKED` and a transactional outbox, no Redis), Data model (platform: `outbox`,
  `jobs`), Hosting and operations (Scaling: workers in the same binary or separately with
  `--role`, scheduled jobs use advisory locks), Integration platform (Events), Decided
  (technical choices); ADR 0012
- **Depends on:** `../m0/02-organizations-and-accounts.md` (`OrgScope`, B9's instance scope for
  job workers), `../m0/08-sessions-and-csrf.md` (B21: the flow cleanup moves onto the jobs
  runner), `../m0/14-compose-deploy.md` (configuration, `serve`)
- **Review:** standard

## Summary

This spec fills the `stepclimb-jobs` placeholder with the background machinery every later M1
spec uses: a job queue in PostgreSQL that workers claim with `FOR UPDATE SKIP LOCKED`, retry with
exponential backoff and hold under a lease (`locked_until`); periodic jobs that run once per
interval across every server; and a transactional outbox, where a change and the event it causes
commit together and a dispatcher later turns each event into jobs for the code that subscribes to
it. Email (spec 12), progress recomputation (spec 05), reminders (spec 13), transcoding (spec 08)
and, in M4, webhooks run on it. It adds no HTTP route.

## Behavior

### Jobs

- **B1.** A job is one `jobs` row: the queue it runs on, its kind (which handler runs it), a JSON
  payload, the time it may run from (`run_at`), how many times it has been claimed (`attempts`),
  how many it may be (`max_attempts`), its lease (`locked_until`), its last error and, for an
  organization's job, the organization (`org_id`). Keys are UUID v7 (`02` B4).
- **B2.** `stepclimb_jobs::enqueue` takes the caller's transaction and inserts the row in it, so a
  job exists exactly when the change that asked for it committed. A caller that wants a job
  without a change passes a transaction of its own. An organization's job is enqueued with the
  caller's `OrgScope`; an instance job (the flow cleanup, the outbox dispatcher, the periodic
  scheduler) with no organization. The payload is at most 64 KiB serialized (larger is a
  programming error: `enqueue` returns an error and the caller's transaction fails); a job that
  needs more stores it in a table and passes the id.
- **B3.** A job may carry a `dedupe_key`. While a job of the same kind and key is waiting (not
  claimed), enqueueing another inserts nothing and moves the waiting job's `run_at` to the
  earlier of the two. A job already claimed does not absorb a new one, so a change made while a
  job runs is never lost: the new job waits and runs after it. Spec 05 uses this to collapse many
  rule changes into one recomputation.
- **B4.** A worker claims up to `n` due jobs of the queues it serves in one statement: rows with
  `run_at <= now`, no live lease (`locked_until` null or `<= now`) and not dead, in `run_at, id`
  order, locked with `FOR UPDATE SKIP LOCKED`, and updated to `attempts = attempts + 1` and
  `locked_until = now + lease`. The claim commits before the handler runs, so a handler's own
  transactions never hold the queue's row locks and two workers never run the same job at once
  while its lease is live. `now` comes from the worker's `Clock` (`stepclimb_core::clock`), passed
  into the query, so tests move time instead of sleeping.
- **B5.** The lease is 5 minutes by default; a kind may set its own. A handler that runs longer
  extends it (`JobContext::extend_lease`), which updates `locked_until` only while the row is
  still this claim's (same id and same `attempts`), and reports a lost lease so the handler can
  stop. A worker that dies leaves the lease to run out; the job is then claimed again and counts
  one more attempt.
- **B6.** A handler returns one of three outcomes:
  - **done:** the worker deletes the row (only while the claim is still its own, as in B5);
  - **retry** with an error: when `attempts < max_attempts` the worker clears the lease, sets
    `last_error` and moves `run_at` to `now + backoff(attempts)`; otherwise the job is dead (B8);
  - **fail** with an error (a payload that does not parse, a row the job names that no longer
    exists, any error that a retry cannot fix): the job is dead at once.
  A handler that panics counts as retry; the panic is caught at the job's task boundary and
  logged.
- **B7.** `backoff(n)` is `min(base * 2^(n-1), cap)` with full jitter (a random delay between half
  that value and that value), `base` 5 s and `cap` 1 h by default; a kind may set its own `base`,
  `cap` and `max_attempts` (default 8, about 10 to 21 minutes of retries in all). The random
  source is passed in, so tests fix it.
- **B8.** A dead job keeps its row with `dead_at` set and its `last_error`, is never claimed
  again, and is logged once at `error` with its kind, id, organization and attempts. Dead rows are
  deleted 30 days after `dead_at` by the periodic `jobs.prune` job. There is no API or UI for
  them in M1 (Out of scope).
- **B9.** `last_error` holds at most 2,048 bytes, cut at a character boundary. Handlers write
  errors that hold no secret, token, password or personal data beyond ids; the error type's
  `Display` is what is stored, so a handler error that wraps a value must leave it out.
- **B10.** A worker runs an organization's job with `OrgScope::instance(org_id)` (`02` B9 lists
  job workers as an allowed call site; one wrapper function in `stepclimb-jobs` carries the
  `#[expect(clippy::disallowed_methods)]`). Before the handler runs, the worker checks that the
  organization exists; a job whose organization is gone fails (B6). An instance job gets no
  scope and must read only instance tables or iterate organizations through
  `organizations::list_ids` (a new instance-wide function, commented as such).
- **B11.** Handlers are registered in code: a registry maps each kind (a `&'static str` in the
  `area.action` form, such as `email.send` or `progress.recompute_course`) to its queue, its
  retry settings and an async function. Enqueueing a kind the registry does not know is an error
  in the enqueueing code's tests (a test walks every `enqueue` call's kind constant); a claimed
  row whose kind this binary does not know (a newer release's, during a rolling deploy) is
  released untouched with its lease cleared and `run_at` moved 1 minute on, without counting the
  attempt, and logged at `warn`.
- **B12.** Jobs run at least once. A handler is written to be safe to run twice (B5's lost lease
  and a crash between the handler's commit and the row's delete both rerun it); each handler's
  spec says how.

### Workers and roles

- **B13.** `stepclimb serve` takes `--role` (also `STEPCLIMB_ROLE`, `role` in the file, ADR
  0022): `all` (the default: HTTP and every queue but those another spec reserves), `web` (HTTP
  only, no worker) or `worker` (every queue, no HTTP listener). Spec 08 adds `transcoder`. Pending
  Q1 for the names. A `worker` process still serves `/healthz` and `/readyz` on the bind address,
  so the container health check works unchanged; it serves nothing else.
- **B14.** Each worker process runs `worker_concurrency` jobs at once
  (`STEPCLIMB_WORKER_CONCURRENCY`, default 4, 1 to 64; a value outside that range stops startup
  naming the setting). When idle it polls for due jobs every second (Q2).
- **B15.** On SIGTERM or SIGINT a worker stops claiming, waits up to 30 s for running jobs, then
  exits; the leases of jobs still running run out and other workers take them (B5).
- **B16.** `/readyz` keeps its meaning (`../m0/14-compose-deploy.md` B17); a worker does not
  report ready until its registry is built.

### Periodic jobs

- **B17.** A periodic job is a registered kind with an interval. The scheduler, run by every
  worker every 10 s, takes `pg_try_advisory_xact_lock` on one constant key in a transaction, reads
  the `job_schedules` rows whose `next_run_at <= now`, enqueues one instance job for each with
  `dedupe_key` set to the schedule's name, moves `next_run_at` to `now + interval`, and commits.
  A server that does not get the lock skips the tick. At startup the server inserts a schedule
  row for every registered periodic kind that has none (`next_run_at = now`) and updates the
  interval of rows whose interval changed; rows for kinds this binary does not know are left
  alone. The advisory lock is the plan's "scheduled jobs use advisory locks".
- **B18.** M1's periodic jobs from this spec: `jobs.prune` (hourly: dead jobs after 30 days, B8;
  dispatched outbox rows after 30 days, B23) and `auth.flows_cleanup` (every 10 minutes: the
  sign-in flow cleanup that `08` B21 runs on a `tokio` interval until the jobs runner exists; this
  spec moves it onto the runner and removes the interval). Other specs add theirs.

### Outbox and event dispatch

- **B19.** An event is one `outbox` row: `org_id`, `topic` (the plan's event names, such as
  `item.completed`), `payload` and `dispatched_at`. `stepclimb_jobs::outbox::emit` takes the
  caller's transaction and the caller's `OrgScope`, so an event exists exactly when the change it
  reports committed.
- **B20.** The stored payload is the event envelope, the shape Standard Webhooks uses for its
  body so M4's webhooks send it unchanged: `{ "type": <topic>, "timestamp": <RFC 3339 UTC, the
  change's clock time>, "data": { ... } }`. The event id is the row's id. `data` is defined by the
  spec that emits the topic, carries ids and the facts of the change, and never a secret or a
  field the event's future receivers could not otherwise read (M4 decides receivers' rights).
  Topics are registered in code with a schema version; a topic's `data` only gains fields within
  one version.
- **B21.** The dispatcher is the instance job kind `outbox.dispatch`, run continuously by the
  workers: it claims up to 100 undispatched rows (`FOR UPDATE SKIP LOCKED`, `id` order), and in
  the same transaction enqueues, for each row and each subscriber registered for its topic, one
  job of the subscriber's kind with payload `{ "event_id": <id> }` and the row's organization,
  then sets `dispatched_at`. An event therefore becomes each subscriber's job exactly once, and a
  subscriber's handler reads the event row by id. A topic with no subscriber is marked dispatched
  with no job.
- **B22.** Delivery order is not guaranteed. Rows are claimed in `id` order (UUID v7, so roughly
  creation order), but two transactions can commit out of order and subscriber jobs retry
  independently; subscribers use the event's `timestamp` and are idempotent per `event_id`.
- **B23.** Dispatched rows are kept 30 days, then deleted by `jobs.prune` (Q3). Undispatched rows
  are never pruned.
- **B24.** M1 registers no production subscriber in this spec; specs 11, 12 and 14 add theirs
  (in-app notifications, email, recurrence), and M4 adds webhooks. Tests register a subscriber of
  their own.

### Logging and limits

- **B25.** Each job logs one line at `debug` when it starts and one at `info` when it ends, with
  the kind, the job id, the organization id, the attempt and the outcome; never the payload.
- **B26.** Every query in `stepclimb-jobs` that reads or writes an organization's rows filters by
  `org_id`; the queue's claim and the dispatcher's claim are instance-wide by design and say so
  in a comment beside the query (`.claude/rules/architecture.md`).

## API

None. The queue and the outbox have no HTTP route in M1, so no route needs a permission, a
four-case test or a cross-organization test. M4 adds the delivery log and replay for webhooks.

Configuration added (ADR 0022 flat keys; every one in `check-config`'s output):

| Variable | File key | Values | Default |
| --- | --- | --- | --- |
| `STEPCLIMB_ROLE` (and `serve --role`) | `role` | `all`, `web`, `worker` | `all` |
| `STEPCLIMB_WORKER_CONCURRENCY` | `worker_concurrency` | 1 to 64 | 4 |

## Data

Migrations `migrations/<version>_jobs.sql` and `migrations/<version>_outbox.sql`, one per issue.
The versions are placeholders here and are fixed when each issue is implemented: each must sort
after every migration on `next` and every open branch at merge time.

```sql
-- <version>_jobs.sql
CREATE TABLE jobs (
    id            uuid        PRIMARY KEY,
    org_id        uuid        REFERENCES organizations (id),   -- null for an instance job (B2)
    queue         text        NOT NULL CHECK (queue ~ '^[a-z][a-z0-9_]{0,31}$'),
    kind          text        NOT NULL CHECK (char_length(kind) BETWEEN 3 AND 100),  -- not in the data model: B11
    payload       jsonb       NOT NULL,
    dedupe_key    text        CHECK (char_length(dedupe_key) BETWEEN 1 AND 200),     -- not in the data model: B3
    run_at        timestamptz NOT NULL,
    attempts      integer     NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    max_attempts  integer     NOT NULL CHECK (max_attempts >= 1),                    -- not in the data model: B7
    locked_until  timestamptz,
    last_error    text        CHECK (octet_length(last_error) <= 2048),
    dead_at       timestamptz,                                                         -- not in the data model: B8
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX jobs_due ON jobs (queue, run_at, id) WHERE dead_at IS NULL;
CREATE UNIQUE INDEX jobs_dedupe_waiting ON jobs (kind, dedupe_key)
    WHERE dedupe_key IS NOT NULL AND locked_until IS NULL AND dead_at IS NULL;
CREATE INDEX jobs_by_org ON jobs (org_id) WHERE org_id IS NOT NULL;

CREATE TABLE job_schedules (                -- not in the data model: B17; instance-wide like plans
    name          text        PRIMARY KEY,
    interval_s    integer     NOT NULL CHECK (interval_s >= 10),
    next_run_at   timestamptz NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);

-- <version>_outbox.sql
CREATE TABLE outbox (
    id             uuid        PRIMARY KEY,
    org_id         uuid        NOT NULL REFERENCES organizations (id),
    topic          text        NOT NULL CHECK (topic ~ '^[a-z][a-z_]*(\.[a-z][a-z_]*)+$'),
    payload        jsonb       NOT NULL CHECK (jsonb_typeof(payload) = 'object'),
    dispatched_at  timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX outbox_undispatched ON outbox (id) WHERE dispatched_at IS NULL;
CREATE INDEX outbox_dispatched ON outbox (dispatched_at) WHERE dispatched_at IS NOT NULL;
CREATE INDEX outbox_by_org ON outbox (org_id, id);
```

`jobs.org_id` is nullable, like `roles.org_id` (`09` Data): an instance job is not organization
data. `job_schedules` holds no organization data and has no `org_id`, like `plans`. A retried
job that would collide with a waiting duplicate under `jobs_dedupe_waiting` is deleted instead,
since the waiting one covers it (B3). The permission suite's cross-organization table check
(`09` B20) lists `jobs` and `outbox` as tables with `org_id` and `job_schedules` as instance-wide.

Repository and crate surface: `stepclimb_jobs::{enqueue, Job, JobContext, Outcome, Registry,
Worker, outbox::{emit, Topic}}`. The SQL lives in `stepclimb-jobs` (it depends on
`stepclimb-core` for `OrgScope`, `Clock` and ids); `bin/check-layering.sh` already allows it.

## Permissions

None added. No route, no catalog entry, no entitlement feature or limit.

## Test cases

Database tests use `#[sqlx::test]` and a `ManualClock`; workers in tests run one claim loop
iteration at a time through a test hook rather than on timers.

- **T1** (B2). A job enqueued in a transaction that rolls back does not exist; one enqueued in a
  transaction that commits does, with `attempts` 0 and its organization.
- **T2** (B3). Two enqueues of `kind` `k` with key `x` while the first waits leave one row with
  the earlier `run_at`; after the row is claimed, a third enqueue inserts a second row.
- **T3** (B4). Two workers on two connections claim from 10 due jobs, 5 each, with no job
  claimed twice; a job with `run_at` one second in the future is not claimed until the clock
  moves past it.
- **T4** (B5). A job whose handler outlives the lease without extending it is claimed again after
  the clock passes `locked_until`; the first handler's `extend_lease` and its done-delete then
  find the claim lost and change nothing.
- **T5** (B6, B7). A handler that returns retry three times runs at `run_at` values within the
  jitter bounds of 5, 10 and 20 s (fixed random source); `last_error` holds the last message.
- **T6** (B6, B8). A job whose handler fails, and one that retries `max_attempts` times, each end
  with `dead_at` set, are never claimed again and are deleted by `jobs.prune` 30 days later, not
  29.
- **T7** (B6). A handler that panics is retried, and the worker keeps claiming other jobs.
- **T8** (B9). A 10 KiB error message is stored cut to at most 2,048 bytes at a character
  boundary (a multibyte character at the cut).
- **T9** (B10). A job enqueued for org A runs with a scope for org A: a handler that reads
  through a repository function sees only org A's rows while org B has rows of the same shape.
  A job whose organization row is deleted before it runs dies with a `fail` outcome.
- **T10** (B11). A claimed row whose kind is unknown is released with `attempts` unchanged and
  `run_at` 1 minute on; the registry test fails when a kind constant used in an `enqueue` call is
  missing from the registry (checked once by removing a registration, as a mutation).
- **T11** (B13, B14). `check-config` prints `role` and `worker_concurrency`; `role = "other"` and
  `worker_concurrency = 0` each exit 2 naming the setting; `--role web` runs no claim loop and
  `--role worker` answers `/healthz` and 404 for `/api/v1/me`.
- **T12** (B15). With a job running, SIGTERM stops new claims and the process exits after the job
  finishes; a job still running at 30 s is left with its lease (paused `tokio` time).
- **T13** (B17). Two schedulers ticking at once enqueue one `jobs.prune` per interval; a schedule
  row is created at startup for a new periodic kind, and its interval updated when the code's
  changes.
- **T14** (B18). An expired sign-in flow is deleted by the `auth.flows_cleanup` job, and the
  server no longer starts the `tokio` interval (`08` B21's tests move to the job).
- **T15** (B19). An event emitted in a transaction that rolls back does not exist.
- **T16** (B20, B21). With two subscribers to `test.happened` and none to `test.ignored`, one
  dispatch of three `test.happened` rows and one `test.ignored` row enqueues six jobs, each with
  `{ "event_id" }` and the row's organization, and marks all four rows dispatched. A second
  dispatch enqueues nothing. Two dispatchers on two connections enqueue each job once.
- **T17** (B20). The stored envelope has `type`, `timestamp` (the manual clock's time, RFC 3339
  UTC) and `data`; the event id is the row id.
- **T18** (B23). Dispatched rows older than 30 days are pruned; an undispatched row 40 days old
  is kept.
- **T19** (B25). A captured log of a job run holds the kind, id, organization and outcome, and no
  payload value (a payload carrying a marker string is not in the log).

## Out of scope

- Webhook delivery, its log and replay: M4 (`docs/specs/m4.md`).
- An admin or operator view of jobs and dead jobs, and Prometheus metrics for the queue: M4
  (metrics) and M6 (operator console).
- Email, push and every other handler: their own specs (12 email, 11 notifications, 08
  transcoding, 05 progress recomputation, 13 reminders, 14 recurrence).
- LISTEN/NOTIFY wake-ups: not built (Q2).

## Open questions

- **Q1. The `--role` values.** The plan names `--role` and `--role transcoder` and no other
  value. Options: (a) `all` (default), `web`, `worker`, plus spec 08's `transcoder`; (b) a list,
  such as `--role web,worker`; (c) separate subcommands (`stepclimb worker`). Recommendation: (a);
  one value per process covers the plan's split (stateless web, separate workers, a CPU-limited
  transcoder) and keeps `serve` the only long-running command.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q2. How idle workers learn about new jobs.** Options: (a) poll every second; (b) LISTEN/NOTIFY
  with a 5 s fallback poll, which needs one dedicated connection per worker process; (c) poll
  with a configurable interval. Recommendation: (a). At 1 s the queue adds at most one cheap
  indexed query per second per process, the latency is invisible for email, events and
  recomputation, and there is no extra connection or reconnect logic.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. How long dispatched events and dead jobs are kept.** The plan sets webhook retries to
  24 h and says nothing about keeping events. Options: (a) 30 days for both; (b) 7 days; (c) a
  setting. Recommendation: (a). It covers M4's 24 h retries and replay with margin, and keeps the
  tables bounded; a setting can come with M4's retention settings.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q4. Retry defaults.** The plan names exponential backoff and no numbers. Options: (a) base
  5 s, cap 1 h, 8 attempts, full jitter, each kind may override; (b) a fixed 1-minute delay, 5
  attempts. Recommendation: (a); kinds such as M4's webhooks set their own to reach the plan's
  24 h.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.

## Issues

1. **I1. Job queue core.** Migration `<version>_jobs.sql` (`jobs` only), `enqueue` with dedupe,
   the claim, lease extension, outcomes, backoff with an injected random source, dead jobs, the
   registry and the scoped run of an organization's job. Covers B1 to B12, B25, B26; T1 to T10,
   T19. Depends on: none (M0 complete).
2. **I2. Worker runtime and roles.** The claim loop with `worker_concurrency`, polling, graceful
   shutdown, `--role` and `STEPCLIMB_ROLE` in the configuration loader and `check-config`, and the
   worker-only listener for the probes. Covers B13 to B16; T11, T12. Depends on: I1.
3. **I3. Periodic jobs.** `job_schedules` (in I3's own migration), the scheduler under the
   advisory lock, startup sync of schedule rows, `jobs.prune` for dead jobs, and moving the
   sign-in flow cleanup from its `tokio` interval onto `auth.flows_cleanup` (updating `08` B21 in
   the same PR). Covers B17, B18; T13, T14, T6 (prune). Depends on: I2.
4. **I4. Outbox and dispatch.** Migration `<version>_outbox.sql`, `outbox::emit`, topic
   registration with schema versions, the `outbox.dispatch` job, subscriber registration, and
   pruning dispatched rows in `jobs.prune`. Covers B19 to B24; T15 to T18. Depends on: I3.
