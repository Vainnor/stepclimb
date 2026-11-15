// SPDX-License-Identifier: AGPL-3.0-or-later
//! The server's log subscriber (plan, Technical -> Hosting and operations: JSON logs).
//!
//! The `log` setting is an `EnvFilter` an operator chooses, and it may turn on `trace` for
//! everything. Below it sits a floor no setting lifts: `webauthn-rs` logs a passkey ceremony's
//! state (its challenge) and the browser's response at `trace` and `debug`, so events from the
//! `webauthn_rs` crates are kept only at `info` and above (docs/specs/m0/04-local-accounts.md
//! B24, docs/specs/m0/08-sessions-and-csrf.md B4).

use tracing::Subscriber;
use tracing_subscriber::{
    EnvFilter, Layer,
    filter::{LevelFilter, Targets},
    fmt::{self, MakeWriter},
    layer::SubscriberExt,
};

/// The log targets held at `info` whatever the setting says. A target matches by prefix, so
/// `webauthn_rs` also covers `webauthn_rs_core` and `webauthn_rs_proto`.
pub const WEBAUTHN_TARGETS: [&str; 2] = ["webauthn_rs", "webauthn_rs_core"];

/// JSON lines to `writer`, filtered by `setting` (an `EnvFilter`, which `Config` has already
/// validated) and then by the floor of [`WEBAUTHN_TARGETS`].
pub fn subscriber<W>(setting: &str, writer: W) -> impl Subscriber + Send + Sync + 'static
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    let floor = WEBAUTHN_TARGETS.iter().fold(
        Targets::new().with_default(LevelFilter::TRACE),
        |t, target| t.with_target(*target, LevelFilter::INFO),
    );
    let layer = fmt::layer()
        .json()
        .with_writer(writer)
        .with_filter(EnvFilter::new(setting))
        .with_filter(floor);
    tracing_subscriber::registry().with(layer)
}
