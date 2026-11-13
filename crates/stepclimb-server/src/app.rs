// SPDX-License-Identifier: AGPL-3.0-or-later
//! The HTTP application.

use axum::{Router, http::header, response::IntoResponse, routing::get};
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::clock::Clock;
use tower_http::trace::TraceLayer;
use utoipa_scalar::{Scalar, Servable};

use crate::{error::ApiError, health, openapi, spa};

/// What every handler can reach.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    /// The time every expiry and lifetime rule reads; tests pass a manual clock.
    pub clock: Clock,
    /// The AES-256-GCM keys for secrets the server reads back, from configuration.
    pub keys: KeyRing,
}

/// The whole application: probes, the API document and its UI, and the SPA for the rest. An
/// unknown `/api/` path and a known path with the wrong method answer problem details.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        .route("/api/openapi.json", get(openapi_json))
        .merge(Scalar::with_url("/docs", openapi::document()))
        // Applies to the routes above, so it stays after the last of them.
        .method_not_allowed_fallback(method_not_allowed)
        .fallback(spa::serve)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// A matched path with another method: 405 as problem details. axum still sets `Allow`.
async fn method_not_allowed() -> ApiError {
    ApiError::method_not_allowed()
}

async fn openapi_json() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/json")],
        openapi::document_json(),
    )
}
