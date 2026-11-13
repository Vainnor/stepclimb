// SPDX-License-Identifier: AGPL-3.0-or-later
//! The HTTP application.

use axum::{Router, http::header, response::IntoResponse, routing::get};
use sqlx::PgPool;
use tower_http::trace::TraceLayer;
use utoipa_scalar::{Scalar, Servable};

use crate::{health, openapi, spa};

/// What every handler can reach.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
}

/// The whole application: probes, the API document and its UI, and the SPA for the rest.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        .route("/api/openapi.json", get(openapi_json))
        .merge(Scalar::with_url("/docs", openapi::document()))
        .fallback(spa::serve)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn openapi_json() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/json")],
        openapi::document_json(),
    )
}
