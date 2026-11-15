// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Stepclimb server: configuration, startup and the HTTP application.
//!
//! The `stepclimb` binary (`src/main.rs`) is a thin wrapper over this library. Today the
//! application serves the operations endpoints (`/healthz`, `/readyz`), sign-out
//! (`POST /auth/sign-out`, [`auth`]), the caller's organization and its settings under
//! `/api/v1/organization` ([`organization`]), the account tree under `/api/v1/accounts`
//! ([`accounts`]), the permission catalog and roles routes under
//! `/api/v1/admin` ([`roles`], with timestamps from [`timestamp`]), the role assignment
//! routes ([`role_assignments`]), the user admin routes under `/api/v1/users` ([`users`]),
//! users' identities under `/api/v1/users/{user_id}/identities` and `/api/v1/me/identities`
//! ([`identities`]), the signed-in user's passkeys under `/api/v1/me/passkeys`
//! ([`my_passkeys`]), the signed-in user at
//! `GET /api/v1/me` ([`me`]) and their appearance choice at `PUT /api/v1/me/appearance`
//! ([`appearance`]), the organization's branding at `/api/v1/admin/branding` and its
//! stylesheet at `/theme.css` ([`branding`]), the OpenAPI document (`/api/openapi.json`) and its Scalar UI
//! (`/docs`), and the built web SPA for every other path, its `index.html` linking the
//! fingerprinted stylesheet and carrying the appearance ([`spa`]). Every `/api/v1` response carries
//! `Cache-Control: no-store` ([`app`]). The other M0 specs in `docs/specs/m0/` add their
//! `/api/v1` endpoints on the conventions in [`error`] (problem details), [`extract`]
//! (extractors that reject with them) and [`pagination`] (cursor pages), authenticate them with
//! [`session`] (the browser session cookie and the `Authenticated` extractor) within the
//! organization [`request_org`] resolves (whose `RequestOrg` extractor gives unauthenticated
//! routes their organization), and protect them with [`require`] (`Require<P, T>`, the
//! permission check, and `Require<F>`, the feature check of [`entitlements`], which also
//! holds the limit check). A sign-in that
//! spans several requests keeps its state in a [`flow`]; sign-in with an outside identity
//! provider starts at [`provider_sign_in`], whose requests to the provider go through
//! [`providers`]. Every unsafe request passes [`csrf`]
//! first. Sign-in and token routes declare a [`rate_limit`] budget, counted per client address
//! as [`client_ip`] finds it; a route that changes roles, assignments, accounts or the
//! organization records that address in the change's audit entry ([`audit`]).

pub mod accounts;
pub mod admin;
pub mod admission;
pub mod app;
pub mod appearance;
pub mod audit;
pub mod auth;
pub mod branding;
pub mod client_ip;
pub mod config;
mod cookie;
pub mod csrf;
pub mod enrollment;
pub mod entitlements;
pub mod error;
pub mod extract;
pub mod flow;
pub mod health;
pub mod healthcheck;
pub mod identities;
pub mod local_sign_in;
pub mod logging;
pub mod me;
pub mod my_passkeys;
pub mod openapi;
pub mod organization;
pub mod pagination;
pub mod passkey_enrollment;
pub mod passkey_options;
pub mod passkey_sign_in;
pub mod provider_sign_in;
pub mod providers;
pub mod rate_limit;
pub mod recovery;
pub mod request_org;
pub mod require;
pub mod role_assignments;
pub mod roles;
pub mod session;
pub mod spa;
pub mod startup;
pub mod timestamp;
pub mod users;
pub mod web_origin;

pub use app::{AppState, router};
pub use config::Config;
