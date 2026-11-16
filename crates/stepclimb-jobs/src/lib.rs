// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Postgres job queue (`FOR UPDATE SKIP LOCKED`) and the transactional outbox, and later
//! email, webhooks, push and auto-submit.
//!
//! Today it holds the queue core of `docs/specs/m1/01-job-queue-and-outbox.md` (issue I1):
//! [`enqueue`] inserts a job in the caller's transaction, with dedupe; [`queue::claim`] takes
//! due jobs under a lease; a [`Worker`] runs each claimed job's handler from the [`Registry`] on
//! a task of its own, with its organization's scope, and records its [`Outcome`]: deleted when
//! done, retried after [`backoff::backoff`], or dead. [`Worker::run`] is the loop a server runs
//! (issue I2): up to `worker_concurrency` jobs at once, a poll every second when idle, and a
//! graceful stop. [`schedule`] runs periodic kinds (issue I3) under an advisory lock, and
//! [`prune`] is the first of them. [`outbox`] (issue I4) writes events in the caller's
//! transaction, and its dispatcher turns each into one job per subscriber of its topic.
//!
//! Jobs run at least once (B12): a lost lease or a crash between a handler's commit and the
//! row's delete runs a handler again, so every handler is written to be safe to run twice.

pub mod backoff;
pub mod kind;
pub mod outbox;
pub mod prune;
pub mod queue;
mod registry;
mod runtime;
pub mod schedule;
mod worker;

pub use kind::{Kind, Retry};
pub use queue::{Enqueued, NewJob, enqueue};
pub use registry::{HandlerFuture, MIN_INTERVAL, Registry, RegistryError};
pub use runtime::{POLL_INTERVAL, RunOptions, SHUTDOWN_GRACE, Stopped};
pub use worker::{Job, JobContext, Lease, Outcome, UNKNOWN_KIND_DELAY, Worker};
