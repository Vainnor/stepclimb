// SPDX-License-Identifier: AGPL-3.0-or-later
//! Identity providers (local accounts with passkeys, OIDC, generic OAuth 2), admission, sessions
//! and CSRF, API keys, and the OAuth 2.1 server for Stepclimb's own apps.
//!
//! Today it holds the foundations of `docs/specs/m0/08-sessions-and-csrf.md`: [`token`], random
//! tokens and API keys with the SHA-256 hashes the server stores, and [`encryption`], the
//! AES-256-GCM key ring for secrets the server reads back. Sessions, CSRF, API-key
//! authentication, the sign-in methods and the OAuth 2.1 server arrive with the rest of that spec
//! and specs 04 to 07.

pub mod encryption;
pub mod token;
