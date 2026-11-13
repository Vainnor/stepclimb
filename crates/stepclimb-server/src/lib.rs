// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Stepclimb server: configuration, startup and the HTTP application.
//!
//! The `stepclimb` binary (`src/main.rs`) is a thin wrapper over this library. Today the
//! application serves the operations endpoints (`/healthz`, `/readyz`), sign-out
//! (`POST /auth/sign-out`, [`auth`]), the OpenAPI document (`/api/openapi.json`) and its Scalar
//! UI (`/docs`), and the built web SPA for every other path. No `/api/v1` endpoint exists yet;
//! the M0 specs in `docs/specs/m0/` add them on the conventions in [`error`] (problem details),
//! [`extract`] (extractors that reject with them) and [`pagination`] (cursor pages), and
//! authenticate them with [`session`] (the browser session cookie and the `Authenticated`
//! extractor) within the organization [`request_org`] resolves. Every unsafe request passes
//! [`csrf`] first.

pub mod app;
pub mod auth;
pub mod config;
pub mod csrf;
pub mod error;
pub mod extract;
pub mod health;
pub mod healthcheck;
pub mod openapi;
pub mod pagination;
pub mod request_org;
pub mod session;
pub mod spa;
pub mod startup;
pub mod web_origin;

pub use app::{AppState, router};
pub use config::Config;
