// SPDX-License-Identifier: AGPL-3.0-or-later
//! The panic hook of `stepclimb serve` (docs/specs/m1/01-job-queue-and-outbox.md B25).
//!
//! Rust's default hook prints a panic's message to stderr. A job handler's panic message can
//! quote the job's payload, which no log line may hold, so `serve` replaces the hook with one
//! that logs through `tracing`, at `error`, only that a thread panicked and where: the source
//! file and line, never the message. The job worker logs which job's handler it was.

use std::panic::PanicHookInfo;

/// Replace the process's panic hook with [`report`].
pub fn install() {
    std::panic::set_hook(Box::new(report));
}

/// Log that a panic happened and its source location, without its message.
pub fn report(info: &PanicHookInfo<'_>) {
    let location = info
        .location()
        .map(|at| format!("{}:{}", at.file(), at.line()));
    tracing::error!(
        location = location.as_deref().unwrap_or("unknown"),
        "a thread panicked; its message is left out of the log, since it can hold job data"
    );
}
