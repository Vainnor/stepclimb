// SPDX-License-Identifier: AGPL-3.0-or-later
//! `serve`'s panic hook logs that a thread panicked and where, never the panic's message, which
//! can quote a job's payload (docs/specs/m1/01-job-queue-and-outbox.md B25). Each test runs in a
//! process of its own under nextest, so replacing the process's hook touches no other test.

use std::io::Write;
use std::sync::{Arc, Mutex};

use stepclimb_server::panic_hook;

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("the log buffer")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_panic_is_logged_without_its_message() {
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    panic_hook::install();
    let marker = "payload-marker-31f7";
    let result = tracing::subscriber::with_default(subscriber, || {
        std::panic::catch_unwind(|| panic!("a handler bug on {{\"note\":\"{marker}\"}}"))
    });
    let _ = std::panic::take_hook();
    assert!(result.is_err(), "the closure panicked");
    let log =
        String::from_utf8(captured.0.lock().expect("the log buffer").clone()).expect("UTF-8 logs");
    assert!(log.contains("ERROR"), "{log}");
    assert!(log.contains("a thread panicked"), "{log}");
    assert!(
        log.contains("tests/panic_hook.rs:"),
        "the location is logged: {log}"
    );
    assert!(!log.contains(marker), "the message is not: {log}");
    assert!(!log.contains("a handler bug"), "{log}");
}
