// SPDX-License-Identifier: AGPL-3.0-or-later
//! The panic hook of `stepclimb serve` (docs/specs/m1/01-job-queue-and-outbox.md B25).
//!
//! Rust's default hook prints a panic's message to stderr. A job handler's panic message can
//! quote the job's payload, which no log line may hold, so `serve` replaces the hook with one
//! that logs through `tracing`, at `error`, that a thread panicked, the thread's name, the source
//! file and line, and a backtrace when `RUST_BACKTRACE` asks for one, never the message. The job
//! worker logs which job's handler it was.

use std::backtrace::Backtrace;
use std::panic::PanicHookInfo;

/// Replace the process's panic hook with [`report`].
pub fn install() {
    std::panic::set_hook(Box::new(report));
}

/// Log that a panic happened, on which thread and where, without its message. The backtrace
/// is captured as `RUST_BACKTRACE` (or `RUST_LIB_BACKTRACE`) says; frames name functions and
/// files, not values.
pub fn report(info: &PanicHookInfo<'_>) {
    let location = info
        .location()
        .map(|at| format!("{}:{}", at.file(), at.line()));
    let thread = std::thread::current();
    tracing::error!(
        thread = thread.name().unwrap_or("unnamed"),
        location = location.as_deref().unwrap_or("unknown"),
        backtrace = %Backtrace::capture(),
        "a thread panicked; its message is left out of the log, since it can hold job data"
    );
}
