// SPDX-License-Identifier: AGPL-3.0-or-later
//! Identity providers (local accounts with passkeys, OIDC, generic OAuth 2), admission, sessions
//! and CSRF, API keys, and the OAuth 2.1 server for Stepclimb's own apps.
//!
//! Today it holds the foundations of `docs/specs/m0/08-sessions-and-csrf.md`: [`token`], random
//! tokens and API keys with the SHA-256 hashes the server stores, and [`encryption`], the
//! AES-256-GCM key ring for secrets the server reads back. For the sign-in providers of specs 05
//! and 06 it has [`outbound`], the HTTP client and URL policy for every request to a URL an admin
//! entered, [`oidc_config`] and [`oauth2_config`], the validated `oidc` and `oauth2` provider
//! configs over what they share ([`provider_config`]: problems, the sealed client secret),
//! [`oidc`], discovery and the authorization request of OIDC sign-in, [`oauth2_flow`], the
//! authorization request, code exchange and userinfo call of generic OAuth 2 sign-in, [`claims`], the claim
//! mapping of both kinds, and [`json_pointer`], its pointers. For local
//! accounts it has [`passkey`], the WebAuthn relying party that registers passkeys
//! (`docs/specs/m0/04-local-accounts.md` B8), and [`password`], the password policy and its
//! argon2id hashes, and [`totp`], TOTP codes and recovery codes (B19 to B21). For the OAuth 2.1
//! server for Stepclimb's own apps it has [`oauth_server`], the client registry and the checks of
//! an authorization request (spec 08 B30, B31). For the admission webhook it has [`webhook`], the
//! Standard Webhooks signing secret (`docs/specs/m0/07-admission.md` B24). Sessions, CSRF and API-key authentication live in
//! the server crate; the OAuth 2.1 token endpoint arrives with spec 08 I8.

pub mod claims;
pub mod encryption;
pub mod json_pointer;
pub mod oauth2_config;
pub mod oauth2_flow;
pub mod oauth_server;
pub mod oidc;
pub mod oidc_config;
pub mod outbound;
pub mod passkey;
pub mod password;
pub mod provider_config;
pub mod token;
pub mod totp;
pub mod webhook;
