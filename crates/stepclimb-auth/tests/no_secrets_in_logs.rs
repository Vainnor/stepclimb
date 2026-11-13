// SPDX-License-Identifier: AGPL-3.0-or-later
//! `docs/specs/m0/08-sessions-and-csrf.md` B4, T3 as far as this crate goes today: tokens, API
//! keys, their hashes, the key ring and every error this crate returns, logged through `tracing`
//! at every level as `?field`, `{:?}` and `{}`, leave no secret in the output. The full-flow T3
//! (a sign-in, an API-key request and a token exchange) runs once those routes exist.

use std::io::Write;
use std::sync::{Arc, Mutex};

use stepclimb_auth::encryption::KeyRing;
use stepclimb_auth::token::{ApiKey, Token};
use tracing::Level;

const KEY_MATERIAL: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

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

fn capture(log: impl FnOnce()) -> String {
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, log);
    let bytes = captured.0.lock().expect("the log buffer").clone();
    String::from_utf8(bytes).expect("UTF-8 logs")
}

#[test]
fn no_level_logs_a_token_key_or_key_material() {
    let token = Token::generate().expect("a token");
    let api_key = ApiKey::generate().expect("an API key");
    let ring = KeyRing::parse(&format!("k1:{KEY_MATERIAL}")).expect("a key ring");
    let sealed = ring.encrypt(b"provider client secret").expect("sealed");
    let mut altered = sealed.clone();
    *altered.last_mut().expect("a tag") ^= 1;
    let decrypt_error = ring.decrypt(&altered).expect_err("altered bytes fail");
    let ring_error = KeyRing::parse(&format!("k1:{KEY_MATERIAL},k2:{}!", &KEY_MATERIAL[..20]))
        .expect_err("a bad entry");
    let presented = format!("{}x", api_key.expose_secret());
    let format_error = ApiKey::parse(&presented).expect_err("a wrong length");

    let output = capture(|| {
        tracing::trace!(?token, ?api_key, hash = ?token.hash(), "trace");
        tracing::debug!(?ring, error = %decrypt_error, "debug");
        tracing::info!(error = %ring_error, debug = ?ring_error, "info");
        tracing::warn!(error = %format_error, "warn {:?} {:?}", token, api_key.hash());
        tracing::error!("error {ring:?} {decrypt_error:?}");
    });

    for level in ["TRACE", "DEBUG", "INFO", "WARN", "ERROR"] {
        assert!(
            output.contains(level),
            "{level} was not captured:\n{output}"
        );
    }
    assert!(output.contains("<redacted>"), "{output}");
    for secret in [
        token.expose_secret(),
        api_key.expose_secret(),
        &api_key.expose_secret()[3..46],
        KEY_MATERIAL,
        &KEY_MATERIAL[..20],
        "provider client secret",
        // The key bytes 0x00..0x1f as a Debug array or in hex.
        "0, 1, 2, 3",
        "000102030405",
    ] {
        assert!(!output.contains(secret), "{secret} leaked into:\n{output}");
    }
}
