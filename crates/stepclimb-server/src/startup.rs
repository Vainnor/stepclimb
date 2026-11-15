// SPDX-License-Identifier: AGPL-3.0-or-later
//! `stepclimb serve` (connect, migrate, sync the built-in roles and the `unlimited` plan, give
//! every organization a plan and its local provider row, start the sign-in flow cleanup,
//! listen) and `stepclimb migrate` (connect, migrate, exit).

use std::future::Future;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime};

use axum::Router;

use sqlx::{PgPool, migrate::Migrator, postgres::PgPoolOptions};
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope,
    clock::Clock,
    db::MIGRATOR,
    id::Uuid,
    local_sign_in, org_plans, organizations,
    plans::{self, UnknownKeys},
    roles,
};
use tokio::net::TcpListener;

use crate::{AppState, Config, flow, providers::Providers, rate_limit::RateLimiter, router};

/// How long a request waits for a database connection from the pool.
pub const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

/// Why the server stopped before or while serving.
#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error("could not connect to the database: {0}")]
    Connect(#[source] sqlx::Error),
    #[error("could not apply the database migrations: {0}")]
    Migrate(#[source] sqlx::migrate::MigrateError),
    #[error("could not write the built-in roles: {0}")]
    SyncRoles(#[source] roles::Error),
    #[error("could not write the unlimited plan: {0}")]
    SyncPlans(#[source] plans::Error),
    #[error("could not list the organizations: {0}")]
    ListOrganizations(#[source] organizations::Error),
    #[error("could not give organization {org_id} its plan: {source}")]
    OrgPlan {
        org_id: Uuid,
        source: org_plans::Error,
    },
    #[error("could not give organization {org_id} its local provider row: {source}")]
    LocalProvider {
        org_id: Uuid,
        source: local_sign_in::Error,
    },
    #[error("could not listen on {addr}: {source}")]
    Bind {
        addr: std::net::SocketAddr,
        source: std::io::Error,
    },
    #[error("the HTTP server failed: {0}")]
    Serve(#[source] std::io::Error),
    #[error(transparent)]
    ProviderClient(#[from] stepclimb_auth::outbound::BuildError),
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
    run_migrations(&MIGRATOR, pool).await
}

/// Apply every migration of `migrator` the database does not have yet.
///
/// sqlx holds a PostgreSQL advisory lock for the whole run, so a second process starting at the
/// same time waits, then finds nothing left to apply. Each migration runs in its own transaction
/// together with its row in `_sqlx_migrations`, so a failing one is rolled back and the error
/// names its version.
pub async fn run_migrations(migrator: &Migrator, pool: &PgPool) -> Result<(), StartupError> {
    migrator.run(pool).await.map_err(StartupError::Migrate)
}

/// Bring the database to what this binary serves: apply the pending migrations, write the
/// built-in role rows (spec 09 B6) and the `unlimited` plan (spec 10 B4) from code, log the plan
/// keys this binary does not know (spec 10 B7), give every organization without a plan the
/// `unlimited` one (spec 10 B5), and give every organization without one its local provider row
/// (spec 04 B1). `now` stamps the rows it writes.
pub async fn prepare(pool: &PgPool, clock: &Clock) -> Result<(), StartupError> {
    migrate(pool).await?;
    tracing::info!("database migrations are up to date");
    let now = clock.now();
    roles::sync_builtin(pool, now)
        .await
        .map_err(StartupError::SyncRoles)?;
    tracing::info!("built-in roles are up to date");
    plans::sync_unlimited(pool, now)
        .await
        .map_err(StartupError::SyncPlans)?;
    tracing::info!("the unlimited plan is up to date");
    let unknown = plans::unknown_keys(pool)
        .await
        .map_err(StartupError::SyncPlans)?;
    warn_about_unknown_plan_keys(&unknown);
    ensure_org_plans(pool, now).await?;
    ensure_local_rows(pool, now).await
}

/// Give every organization without a local provider row its row (spec 04 B1): one created before
/// the local provider migration has none. A failure stops startup: local sign-in would fail for
/// that organization.
async fn ensure_local_rows(pool: &PgPool, now: SystemTime) -> Result<(), StartupError> {
    let ids = organizations::ids(pool)
        .await
        .map_err(StartupError::ListOrganizations)?;
    let mut given = 0_usize;
    for org_id in ids {
        let inserted = local_sign_in::ensure_row(pool, startup_scope(org_id), now)
            .await
            .map_err(|source| StartupError::LocalProvider { org_id, source })?;
        given += usize::from(inserted);
    }
    if given > 0 {
        tracing::info!(
            organizations = given,
            "gave organizations their local provider row"
        );
    }
    Ok(())
}

/// Give every organization without an `org_plans` row the `unlimited` plan (spec 10 B5). A
/// failure for one organization stops startup: serving it without a plan would fail every
/// entitlement check.
async fn ensure_org_plans(pool: &PgPool, now: SystemTime) -> Result<(), StartupError> {
    let ids = organizations::ids(pool)
        .await
        .map_err(StartupError::ListOrganizations)?;
    let mut given = 0_usize;
    for org_id in ids {
        let inserted = org_plans::ensure_selfhost(pool, startup_scope(org_id), now)
            .await
            .map_err(|source| StartupError::OrgPlan { org_id, source })?;
        given += usize::from(inserted);
    }
    if given > 0 {
        tracing::info!(
            organizations = given,
            "gave organizations the unlimited plan"
        );
    }
    Ok(())
}

#[expect(
    clippy::disallowed_methods,
    reason = "spec 02 B9 allows the instance scope in a startup task; spec 10 B5 gives each organization its plan before serving"
)]
fn startup_scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// Log a warning for each plan row holding a feature or limit key this binary does not know,
/// which reading the plan ignores (spec 10 B7).
pub fn warn_about_unknown_plan_keys(unknown: &[UnknownKeys]) {
    for row in unknown {
        tracing::warn!(
            plan = %row.plan,
            features = ?row.features,
            limits = ?row.limits,
            "plan {:?} holds keys this release does not know; they are ignored",
            row.plan
        );
    }
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

/// Log a warning when the insecure-providers switch is on: the server then fetches `http` and
/// loopback URLs an admin enters for an identity provider, which only tests and local
/// development need (spec 05 Q4).
pub fn warn_about_insecure_providers(config: &Config) {
    if config.allow_insecure_providers {
        tracing::warn!(
            "allow_insecure_providers is on: identity providers may use http and loopback \
             addresses; never turn it on in production"
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
        public_url = %config.public_url,
        allowed_origins = %config.allowed_origins_list(),
        trusted_proxies = %config.trusted_proxies_or_none(),
        sign_in_rate_limit = %config.sign_in_rate_limit,
        token_rate_limit = %config.token_rate_limit,
        failed_sign_in_limit = %config.failed_sign_in_limit,
        allow_insecure_providers = %config.allow_insecure_providers,
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

/// Connect, [`prepare`] the database, then serve HTTP on `config.bind` until SIGINT or SIGTERM.
pub async fn serve(config: Config) -> Result<(), StartupError> {
    log_configuration(&config);
    warn_about_development_keys(&config.encryption_keys);
    warn_about_insecure_providers(&config);
    let pool = connect(&config.database_url).await?;
    let clock = Clock::system();
    prepare(&pool, &clock).await?;
    let listener = TcpListener::bind(config.bind)
        .await
        .map_err(|source| StartupError::Bind {
            addr: config.bind,
            source,
        })?;
    tracing::info!(addr = %config.bind, "listening");
    flow::spawn_cleanup(pool.clone(), clock.clone());
    serve_on(listener, config, pool, clock, router, shutdown_signal()).await
}

/// Build the application state from `config`, `db` and `clock`, with the rate limiter `config`
/// sets, and serve `app(state)` on `listener` until `shutdown` completes. Each request carries
/// its TCP peer's address (`ConnectInfo<SocketAddr>`), which the rate limits start from.
/// [`serve`] passes [`router`]; a test passes a router with a rate-limited route.
pub async fn serve_on(
    listener: TcpListener,
    config: Config,
    db: PgPool,
    clock: Clock,
    app: impl FnOnce(AppState) -> Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), StartupError> {
    let rate_limits = RateLimiter::new(config.limits(), config.trusted_proxies.clone());
    let providers = Providers::new(config.allow_insecure_providers)?;
    let state = AppState {
        db,
        clock,
        keys: config.encryption_keys,
        public_url: config.public_url,
        allowed_origins: config.allowed_origins,
        rate_limits,
        providers,
    };
    axum::serve(
        listener,
        app(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
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
                ("STEPCLIMB_PUBLIC_URL", "https://lms.example.org"),
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
        assert!(
            output.contains("public_url=https://lms.example.org"),
            "{output}"
        );
        assert!(!output.contains("zq"), "{output}");
        assert!(!output.contains("AAEC"), "{output}");
    }

    fn config_with_insecure_providers(value: &str) -> Config {
        Config::load(
            &[
                ("STEPCLIMB_DATABASE_URL", "postgres://app@db:5432/stepclimb"),
                (
                    "STEPCLIMB_ENCRYPTION_KEYS",
                    "k:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
                ),
                ("STEPCLIMB_PUBLIC_URL", "https://lms.example.org"),
                ("STEPCLIMB_ALLOW_INSECURE_PROVIDERS", value),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn the_insecure_providers_switch_is_warned_about_only_when_on() {
        let on = config_with_insecure_providers("true");
        let output = captured(|| warn_about_insecure_providers(&on));
        assert!(output.contains("WARN"), "{output}");
        assert!(
            output.contains("allow_insecure_providers is on"),
            "{output}"
        );
        let off = config_with_insecure_providers("false");
        assert_eq!(captured(|| warn_about_insecure_providers(&off)), "");
    }

    #[test]
    fn unknown_plan_keys_are_warned_about_by_plan() {
        let unknown = [UnknownKeys {
            plan: "test_plan".to_owned(),
            features: vec!["retired_feature".to_owned()],
            limits: vec!["retired_limit".to_owned()],
        }];
        let output = captured(|| warn_about_unknown_plan_keys(&unknown));
        assert!(output.contains("WARN"), "{output}");
        assert!(
            output.contains(r#"plan "test_plan" holds keys this release does not know"#),
            "{output}"
        );
        assert!(output.contains("retired_feature"), "{output}");
        assert!(output.contains("retired_limit"), "{output}");
        assert_eq!(captured(|| warn_about_unknown_plan_keys(&[])), "");
    }

    #[test]
    fn a_ring_without_the_development_key_logs_nothing() {
        let output = warnings_for("prod:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=");
        assert_eq!(output, "");
    }
}
