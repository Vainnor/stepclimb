// SPDX-License-Identifier: AGPL-3.0-or-later
//! The OAuth 2.1 server for Stepclimb's own apps: its clients and its authorization requests
//! (docs/specs/m0/08-sessions-and-csrf.md B30 to B37, Q10 to Q13).
//!
//! [`clients`] is the registry, a fixed list in code (Q10 (a)): public clients only, each with
//! the redirect URIs it may name. [`authorize`] reads an authorization request's query in the
//! order RFC 6749 section 4.1.2.1 requires: the client and its redirect URI first, refused
//! without a redirect when either is wrong, then everything else, refused back to the redirect
//! URI with an RFC 6749 error. Only the authorization code flow with PKCE `S256` passes.
//!
//! [`token_request`] reads the bodies of the token and revocation endpoints and checks a PKCE
//! verifier against its challenge, and [`access_token`] signs and checks the access tokens the
//! token endpoint issues (B32 to B37).
//!
//! Nothing here touches the network, the database or a clock: the server's handler
//! (`stepclimb-server` `oauth_authorize` and `oauth_token`) does that around these functions.

pub mod access_token;
pub mod authorize;
pub mod clients;
pub mod token_request;
