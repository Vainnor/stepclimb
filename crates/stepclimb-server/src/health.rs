// SPDX-License-Identifier: AGPL-3.0-or-later
//! Liveness (`/healthz`) and readiness (`/readyz`) probes (plan, Technical -> Hosting and
//! operations).

use std::time::Duration;

use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;
use utoipa::ToSchema;

use crate::app::AppState;

/// How long `/readyz` waits for the database before answering 503.
pub const READY_TIMEOUT: Duration = Duration::from_secs(2);

/// A probe's answer.
#[derive(Debug, Serialize, ToSchema)]
pub struct Health {
    pub status: HealthStatus,
}

/// `ok` when the probe passed, `unavailable` when it did not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Ok,
    Unavailable,
}

/// The server process is running.
//
// It checks nothing else, so an orchestrator restarts the process only when it stops answering.
#[utoipa::path(
    get,
    path = "/healthz",
    operation_id = "healthz",
    tag = "operations",
    responses((status = 200, description = "The server is running", body = Health))
)]
pub async fn healthz() -> Json<Health> {
    Json(Health {
        status: HealthStatus::Ok,
    })
}

/// The server can take traffic: the database answered within two seconds.
//
// The wait is READY_TIMEOUT.
#[utoipa::path(
    get,
    path = "/readyz",
    operation_id = "readyz",
    tag = "operations",
    responses(
        (status = 200, description = "The database answered", body = Health),
        (status = 503, description = "The database did not answer in time", body = Health)
    )
)]
pub async fn readyz(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    let probe = sqlx::query_scalar!(r#"SELECT 1 AS "one!""#).fetch_one(&state.db);
    match tokio::time::timeout(READY_TIMEOUT, probe).await {
        Ok(Ok(1)) => (
            StatusCode::OK,
            Json(Health {
                status: HealthStatus::Ok,
            }),
        ),
        Ok(Ok(other)) => {
            tracing::error!(
                value = other,
                "readiness probe: SELECT 1 returned another value"
            );
            unavailable()
        }
        Ok(Err(error)) => {
            tracing::warn!(%error, "readiness probe: database query failed");
            unavailable()
        }
        Err(_) => {
            tracing::warn!(timeout = ?READY_TIMEOUT, "readiness probe: database timed out");
            unavailable()
        }
    }
}

fn unavailable() -> (StatusCode, Json<Health>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(Health {
            status: HealthStatus::Unavailable,
        }),
    )
}
