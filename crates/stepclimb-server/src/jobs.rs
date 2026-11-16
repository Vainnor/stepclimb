// SPDX-License-Identifier: AGPL-3.0-or-later
//! The job kinds this binary runs (docs/specs/m1/01-job-queue-and-outbox.md B11, B13).
//!
//! [`registry`] is every kind `stepclimb serve` registers with its handler; the worker of an
//! `all` or `worker` process serves every queue in it. No spec has registered a kind yet; spec
//! m1/01 issue I3 adds the first periodic ones.

use stepclimb_jobs::Registry;

/// The registry a `stepclimb serve` worker runs with.
#[must_use]
pub fn registry() -> Registry {
    Registry::new()
}
