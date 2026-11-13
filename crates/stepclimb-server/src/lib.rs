// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Stepclimb server: configuration, startup and the HTTP application.
//!
//! The `stepclimb` binary (`src/main.rs`) is a thin wrapper over this library. Today the
//! application serves the operations endpoints (`/healthz`, `/readyz`), sign-out
//! (`POST /auth/sign-out`, [`auth`]), the caller's organization and its settings under
//! `/api/v1/organization` ([`organization`]), the account tree under `/api/v1/accounts`
//! ([`accounts`]), the permission catalog and roles routes under
//! `/api/v1/admin` ([`roles`], with timestamps from [`timestamp`]), the role assignment
//! routes ([`role_assignments`]), the signed-in user at
//! `GET /api/v1/me` ([`me`]), the OpenAPI document (`/api/openapi.json`) and its Scalar UI
//! (`/docs`), and the built web SPA for every other path. Every `/api/v1` response carries
//! `Cache-Control: no-store` ([`app`]). The other M0 specs in `docs/specs/m0/` add their
//! `/api/v1` endpoints on the conventions in [`error`] (problem details), [`extract`]
//! (extractors that reject with them) and [`pagination`] (cursor pages), authenticate them with
//! [`session`] (the browser session cookie and the `Authenticated` extractor) within the
//! organization [`request_org`] resolves (whose `RequestOrg` extractor gives unauthenticated
//! routes their organization), and protect them with [`require`] (`Require<P, T>`, the
//! permission check, and `Require<F>`, the feature check of [`entitlements`], which also
//! holds the limit check). A sign-in that
//! spans several requests keeps its state in a [`flow`]. Every unsafe request passes [`csrf`]
//! first. Sign-in and token routes declare a [`rate_limit`] budget, counted per client address
//! as [`client_ip`] finds it; a route that changes roles, assignments, accounts or the
//! organization records that address in the change's audit entry ([`audit`]).

pub mod accounts;
pub mod admin;
pub mod app;
pub mod audit;
pub mod auth;
pub mod client_ip;
pub mod config;
mod cookie;
pub mod csrf;
pub mod entitlements;
pub mod error;
pub mod extract;
pub mod flow;
pub mod health;
pub mod healthcheck;
pub mod me;
pub mod openapi;
pub mod organization;
pub mod pagination;
pub mod rate_limit;
pub mod request_org;
pub mod require;
pub mod role_assignments;
pub mod roles;
pub mod session;
pub mod spa;
pub mod startup;
pub mod timestamp;
pub mod web_origin;

pub use app::{AppState, router};
pub use config::Config;
