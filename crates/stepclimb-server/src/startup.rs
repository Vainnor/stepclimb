// SPDX-License-Identifier: AGPL-3.0-or-later
//! `stepclimb serve` (connect, migrate, listen) and `stepclimb migrate` (connect, migrate, exit).

use std::time::Duration;

use sqlx::{PgPool, postgres::PgPoolOptions};
use stepclimb_auth::encryption::KeyRing;
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

/// Log a warning for each key in `keys` whose bytes are the published development key, so a
/// deployment that copied the Compose example's key says so on every start.
pub fn warn_about_development_keys(keys: &KeyRing) {
    for key_id in keys.development_key_ids() {
        tracing::warn!(
            key_id,
            "encryption key {key_id:?} is the published development key; anything encrypted \
             under it is readable by anyone with the Stepclimb repository"
        );
    }
}

/// Log the configuration's non-secret values. The database URL and the key ring are secrets and
/// are left out.
pub fn log_configuration(config: &Config) {
    tracing::info!(
        file = config.file.as_ref().map(|path| path.display().to_string()),
        bind = %config.bind,
        log = %config.log,
        "configuration loaded"
    );
}

/// `stepclimb migrate`: connect, apply every pending migration, then close the pool.
pub async fn migrate_only(config: &Config) -> Result<(), StartupError> {
    log_configuration(config);
    let pool = connect(&config.database_url).await?;
    migrate(&pool).await?;
    tracing::info!("database migrations are up to date");
    pool.close().await;
    Ok(())
}

/// Connect, migrate, then serve HTTP on `config.bind` until SIGINT or SIGTERM.
pub async fn serve(config: Config) -> Result<(), StartupError> {
    log_configuration(&config);
    warn_about_development_keys(&config.encryption_keys);
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

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use base64::Engine as _;

    use super::*;

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn captured(log: impl FnOnce()) -> String {
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, log);
        String::from_utf8(captured.0.lock().unwrap().clone()).unwrap()
    }

    fn warnings_for(ring: &str) -> String {
        let keys = KeyRing::parse(ring).unwrap();
        captured(|| warn_about_development_keys(&keys))
    }

    #[test]
    fn the_development_key_is_warned_about_by_id() {
        let development = base64::engine::general_purpose::STANDARD
            .encode(stepclimb_auth::encryption::DEVELOPMENT_KEY);
        let output = warnings_for(&format!(
            "prod:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=,old-dev:{development}"
        ));
        assert!(output.contains("WARN"), "{output}");
        assert!(
            output.contains(r#"encryption key "old-dev" is the published development key"#),
            "{output}"
        );
        assert!(!output.contains("prod"), "{output}");
        assert!(!output.contains(&development), "{output}");
    }

    #[test]
    fn the_configuration_log_shows_only_non_secret_values() {
        let config = Config::load(
            &[
                (
                    "STEPCLIMB_DATABASE_URL",
                    "postgres://app:zq-db-secret@db:5432/stepclimb",
                ),
                (
                    "STEPCLIMB_ENCRYPTION_KEYS",
                    "zq:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
                ),
                ("STEPCLIMB_BIND", "0.0.0.0:9000"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect(),
            None,
        )
        .unwrap();
        let output = captured(|| log_configuration(&config));
        assert!(output.contains("configuration loaded"), "{output}");
        assert!(output.contains("bind=0.0.0.0:9000"), "{output}");
        assert!(output.contains("log=info"), "{output}");
        assert!(!output.contains("zq"), "{output}");
        assert!(!output.contains("AAEC"), "{output}");
    }

    #[test]
    fn a_ring_without_the_development_key_logs_nothing() {
        let output = warnings_for("prod:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=");
        assert_eq!(output, "");
    }
}
