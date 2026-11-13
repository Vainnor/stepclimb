// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Stepclimb server: configuration, startup and the HTTP application.
//!
//! The `stepclimb` binary (`src/main.rs`) is a thin wrapper over this library. Today the
//! application serves the operations endpoints (`/healthz`, `/readyz`), sign-out
//! (`POST /auth/sign-out`, [`auth`]), the permission catalog and roles routes under
//! `/api/v1/admin` ([`roles`], with timestamps from [`timestamp`]), the OpenAPI document
//! (`/api/openapi.json`) and its Scalar UI (`/docs`), and the built web SPA for every other
//! path. The other M0 specs in `docs/specs/m0/` add their `/api/v1` endpoints on the
//! conventions in [`error`] (problem details), [`extract`] (extractors that reject with them)
//! and [`pagination`] (cursor pages), authenticate them with [`session`] (the browser session
//! cookie and the `Authenticated` extractor) within the organization [`request_org`] resolves,
//! and protect them with [`require`] (`Require<P, T>`, the permission check). A sign-in that
//! spans several requests keeps its state in a [`flow`]. Every unsafe request passes [`csrf`]
//! first. Sign-in and token routes declare a [`rate_limit`] budget, counted per client address
//! as [`client_ip`] finds it.

pub mod app;
pub mod auth;
pub mod client_ip;
pub mod config;
mod cookie;
pub mod csrf;
pub mod error;
pub mod extract;
pub mod flow;
pub mod health;
pub mod healthcheck;
pub mod openapi;
pub mod pagination;
pub mod rate_limit;
pub mod request_org;
pub mod require;
pub mod roles;
pub mod session;
pub mod spa;
pub mod startup;
pub mod timestamp;
pub mod web_origin;

pub use app::{AppState, router};
pub use config::Config;
