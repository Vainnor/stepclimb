// SPDX-License-Identifier: AGPL-3.0-or-later
//! The mock identity provider over HTTP on 127.0.0.1 (`docs/specs/m0/06-oauth2-and-vatsim-preset.md`
//! T1): what it serves parses with the `openidconnect` crate, a whole sign-in through that crate's
//! client succeeds, every fault switch changes what it names, PKCE and client authentication are
//! enforced, and every request is recorded.

mod common;
mod faults;
mod lifecycle;
mod oauth2;
mod oidc;
mod pkce_and_clients;
