// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Stepclimb server: configuration, startup and the HTTP application.
//!
//! The `stepclimb` binary (`src/main.rs`) is a thin wrapper over this library. Today the
//! application serves the operations endpoints (`/healthz`, `/readyz`), the OpenAPI document
//! (`/api/openapi.json`) and its Scalar UI (`/docs`), and the built web SPA for every other
//! path. No product endpoint exists yet; the M0 specs in `docs/specs/m0/` add them.

pub mod app;
pub mod config;
pub mod health;
pub mod openapi;
pub mod spa;
pub mod startup;

pub use app::{AppState, router};
pub use config::Config;
