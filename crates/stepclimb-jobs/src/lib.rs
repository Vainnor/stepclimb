// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Postgres job queue (`FOR UPDATE SKIP LOCKED`), and later the transactional outbox, email,
//! webhooks, push and auto-submit.
//!
//! Today it holds the queue core of `docs/specs/m1/01-job-queue-and-outbox.md` (issue I1):
//! [`enqueue`] inserts a job in the caller's transaction, with dedupe; [`queue::claim`] takes
//! due jobs under a lease; a [`Worker`] runs each claimed job's handler from the [`Registry`] on
//! a task of its own, with its organization's scope, and records its [`Outcome`]: deleted when
//! done, retried after [`backoff::backoff`], or dead. The worker loop and `--role` (I2), periodic
//! jobs (I3) and the outbox (I4) come next.
//!
//! Jobs run at least once (B12): a lost lease or a crash between a handler's commit and the
//! row's delete runs a handler again, so every handler is written to be safe to run twice.

pub mod backoff;
pub mod kind;
pub mod queue;
mod registry;
mod worker;

pub use kind::{Kind, Retry};
pub use queue::{Enqueued, NewJob, enqueue};
pub use registry::{HandlerFuture, Registry, RegistryError};
pub use worker::{Job, JobContext, Lease, Outcome, UNKNOWN_KIND_DELAY, Worker};
