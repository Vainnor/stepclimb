// SPDX-License-Identifier: AGPL-3.0-or-later
//! `stepclimb serve`: connect, migrate, listen.

use std::time::Duration;

use sqlx::{PgPool, postgres::PgPoolOptions};
use stepclimb_core::{clock::Clock, db::MIGRATOR};
use tokio::net::TcpListener;

use crate::{AppState, Config, router};

/// How long a request waits for a database connection from the pool.
pub const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

/// Why the server stopped before or while serving.
#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error("could not connect to the database: {0}")]
    Connect(#[source] sqlx::Error),
    #[error("could not apply the database migrations: {0}")]
    Migrate(#[source] sqlx::migrate::MigrateError),
    #[error("could not listen on {addr}: {source}")]
    Bind {
        addr: std::net::SocketAddr,
        source: std::io::Error,
    },
    #[error("the HTTP server failed: {0}")]
    Serve(#[source] std::io::Error),
}

/// Open the connection pool.
pub async fn connect(database_url: &str) -> Result<PgPool, StartupError> {
    PgPoolOptions::new()
        .acquire_timeout(ACQUIRE_TIMEOUT)
        .connect(database_url)
        .await
        .map_err(StartupError::Connect)
}

/// Apply every embedded migration the database does not have yet.
pub async fn migrate(pool: &PgPool) -> Result<(), StartupError> {
    MIGRATOR.run(pool).await.map_err(StartupError::Migrate)
}

/// Connect, migrate, then serve HTTP on `config.bind` until SIGINT or SIGTERM.
pub async fn serve(config: Config) -> Result<(), StartupError> {
    let pool = connect(&config.database_url).await?;
    migrate(&pool).await?;
    tracing::info!("database migrations are up to date");
    let listener = TcpListener::bind(config.bind)
        .await
        .map_err(|source| StartupError::Bind {
            addr: config.bind,
            source,
        })?;
    tracing::info!(addr = %config.bind, "listening");
    let state = AppState {
        db: pool,
        clock: Clock::system(),
        keys: config.encryption_keys,
    };
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(StartupError::Serve)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "could not listen for SIGINT");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "could not listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}
