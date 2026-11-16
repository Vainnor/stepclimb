// SPDX-License-Identifier: AGPL-3.0-or-later
//! `stepclimb serve` (connect, migrate, sync the built-in roles and the `unlimited` plan, give
//! every organization a plan and its local provider row, re-render saved branding an older
//! generator version wrote, write the providers of `stepclimb.toml`, start the job worker and
//! the scheduler of periodic jobs when the role runs them, listen) and `stepclimb migrate`
//! (connect, migrate, exit). The branding pass runs in
//! [`prepare`], before the providers: the two touch different rows, and a re-render is correct
//! for this release whether or not a bad `[[providers]]` entry then stops it (an older release
//! started afterwards re-renders the newer record in memory for each read).

use std::future::Future;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime};

use axum::Router;

use sqlx::{PgPool, migrate::Migrator, postgres::PgPoolOptions};
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope,
    branding::{self, Fallback, Rerendered},
    clock::Clock,
    db::MIGRATOR,
    id::Uuid,
    local_sign_in, org_plans, organizations,
    plans::{self, UnknownKeys},
    roles,
    theme::{ContrastReport, RenderError},
};
use tokio::net::TcpListener;

use stepclimb_auth::provider_config::ConfigSecrets;

use stepclimb_jobs::{
    POLL_INTERVAL, Registry, RegistryError, RunOptions, Worker, backoff::Random, outbox, schedule,
};
use tokio::sync::watch;

use crate::{
    AppState, Config,
    app::probes_router,
    config::{STORAGE_URL, StorageConfig},
    config_providers, jobs,
    password_hasher::PasswordHasher,
    providers::Providers,
    rate_limit::RateLimiter,
    router,
    storage::{Storage, StorageError},
};

/// How long a request waits for a database connection from the pool.
pub const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

/// The most connections the HTTP pool opens. The admission webhook's instance-wide limit is half
/// of it (`crate::admission_webhook_limits`). The job worker has a pool of its own
/// ([`connect_jobs`]).
pub const POOL_MAX_CONNECTIONS: u32 = 10;

/// The `application_name` of the job worker's connections, so an operator can tell them apart
/// in `pg_stat_activity`.
pub const JOBS_APPLICATION_NAME: &str = "stepclimb-jobs";

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
    #[error("could not re-render the branding of organization {org_id}: {source}")]
    Branding {
        org_id: Uuid,
        source: branding::Error,
    },
    #[error("could not listen on {addr}: {source}")]
    Bind {
        addr: std::net::SocketAddr,
        source: std::io::Error,
    },
    #[error("could not read the address the listener on {addr} got: {source}")]
    LocalAddr {
        addr: std::net::SocketAddr,
        source: std::io::Error,
    },
    #[error("could not build the job registry: {0}")]
    Registry(#[source] RegistryError),
    #[error("could not write the periodic job schedules: {0}")]
    Schedules(#[source] stepclimb_jobs::queue::Error),
    #[error("could not count the outbox events of unknown topics: {0}")]
    UnknownTopics(#[source] stepclimb_jobs::outbox::Error),
    #[error("the HTTP server failed: {0}")]
    Serve(#[source] std::io::Error),
    #[error(transparent)]
    ProviderClient(#[from] stepclimb_auth::outbound::BuildError),
    #[error(transparent)]
    ConfigProviders(#[from] config_providers::ApplyError),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

impl StartupError {
    /// The process exit code: 2 when a `[[providers]]` entry does not fit the instance, as for
    /// any refused configuration; 1 otherwise.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::ConfigProviders(config_providers::ApplyError::Entry(_)) => 2,
            _ => 1,
        }
    }
}

/// Open the connection pool.
pub async fn connect(database_url: &str) -> Result<PgPool, StartupError> {
    PgPoolOptions::new()
        .max_connections(POOL_MAX_CONNECTIONS)
        .acquire_timeout(ACQUIRE_TIMEOUT)
        .connect(database_url)
        .await
        .map_err(StartupError::Connect)
}

/// The job worker's pool (spec m1/01 B14, B17, B21): one connection per job it may run at once,
/// and headroom of one each for the worker's claims, the scheduler's tick and the outbox
/// dispatcher. The pool is shared, so these are not reserved: a handler that opens a second
/// connection can still make the others wait. The HTTP pool is separate, so neither waits on
/// the other.
pub fn jobs_pool_options(concurrency: std::num::NonZeroU32) -> PgPoolOptions {
    PgPoolOptions::new()
        .max_connections(concurrency.get().saturating_add(3))
        .acquire_timeout(ACQUIRE_TIMEOUT)
}

/// Open the job worker's pool on `database_url` ([`jobs_pool_options`]), its connections named
/// [`JOBS_APPLICATION_NAME`].
pub async fn connect_jobs(
    database_url: &str,
    concurrency: std::num::NonZeroU32,
) -> Result<PgPool, StartupError> {
    let options: sqlx::postgres::PgConnectOptions =
        database_url.parse().map_err(StartupError::Connect)?;
    jobs_pool_options(concurrency)
        .connect_with(options.application_name(JOBS_APPLICATION_NAME))
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
/// (spec 04 B1), and re-render every saved branding another generator version wrote (spec 12
/// B28). `now` stamps the rows it writes.
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
    ensure_local_rows(pool, now).await?;
    refresh_branding(pool).await
}

/// Re-render and store every saved branding an older generator version wrote (spec 12, B28), so
/// each stylesheet declares every token this release's bundle uses and no request renders one.
/// Inputs that fail this generator get the default brand's stylesheet. Every organization whose
/// record holds that fallback, made now or by an earlier start, is logged at warn on each start
/// with the failing pairs, until branding is saved again. A stored record this release cannot
/// read is logged and left: its organization's branding reads answer 500 as before. A database
/// error stops startup.
async fn refresh_branding(pool: &PgPool) -> Result<(), StartupError> {
    let ids = organizations::ids(pool)
        .await
        .map_err(StartupError::ListOrganizations)?;
    let mut rerendered = 0_usize;
    for org_id in ids {
        let scope = startup_scope(org_id);
        let failed = match branding::refresh(pool, scope).await {
            Ok(Some(Rerendered::Passed(_))) => {
                rerendered += 1;
                None
            }
            Ok(Some(Rerendered::FellBack { error, .. })) => {
                rerendered += 1;
                Some(failed_pairs(&error))
            }
            Ok(None) => branding::get(pool, scope)
                .await
                .map(|saved| {
                    saved
                        .and_then(|saved| saved.fallback)
                        .map(|f| stored_pairs(&f))
                })
                .map_err(|source| StartupError::Branding { org_id, source })?,
            Err(error @ branding::Error::StoredBranding(_)) => {
                tracing::error!(%org_id, %error, "saved branding left as stored");
                None
            }
            Err(source) => return Err(StartupError::Branding { org_id, source }),
        };
        if let Some(failed) = failed {
            warn_about_branding_fallback(org_id, &failed);
        }
    }
    if rerendered > 0 {
        tracing::info!(
            organizations = rerendered,
            "re-rendered saved branding with this release's theme generator"
        );
    }
    Ok(())
}

/// The warning for an organization whose saved branding fails this generator, naming it and
/// each failing pair.
fn warn_about_branding_fallback(org_id: Uuid, failed: &[String]) {
    tracing::warn!(
        %org_id,
        ?failed,
        "saved branding fails this release's theme generator; its pages use the default theme \
         until branding is saved again"
    );
}

/// Each failing pair of a contrast refusal as `Light fg-soft on bg-canvas 4.40 < 4.5`; any other
/// refusal as its message.
fn failed_pairs(error: &RenderError) -> Vec<String> {
    match error {
        RenderError::Contrast(reports) => failing(reports),
        other => vec![other.to_string()],
    }
}

/// [`failed_pairs`] for a stored fallback, whose report is empty when the generator refused the
/// inputs.
fn stored_pairs(fallback: &Fallback) -> Vec<String> {
    if fallback.report.is_empty() {
        vec![REFUSED.to_owned()]
    } else {
        failing(&fallback.report)
    }
}

/// [`stored_pairs`] for a fallback without a report.
const REFUSED: &str = "the theme generator refused the saved inputs";

fn failing(reports: &[ContrastReport]) -> Vec<String> {
    reports
        .iter()
        .flat_map(|report| {
            report.pairs.iter().filter(|pair| !pair.pass).map(|pair| {
                format!(
                    "{:?} {} on {} {:.2} < {}",
                    report.appearance, pair.foreground, pair.background, pair.ratio, pair.minimum
                )
            })
        })
        .collect()
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
/// deployment that copied the Compose example's key says so on every start. Any key of the ring
/// verifies the access tokens that name it (spec 08 B46), so the warning says so for a key that
/// no longer encrypts too.
pub fn warn_about_development_keys(keys: &KeyRing) {
    for key_id in keys.development_key_ids() {
        tracing::warn!(
            key_id,
            "encryption key {key_id:?} is the published development key; anything encrypted \
             under it is readable by anyone with the Stepclimb repository, and anyone can sign \
             app access tokens this server accepts with it"
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
        role = %config.role,
        worker_concurrency = %config.worker_concurrency,
        providers = %config.providers_or_none(),
        storage = %config.storage,
        "configuration loaded"
    );
}

/// The store `config` names, opened and probed (docs/specs/m1/06-files.md B1), or `None`
/// after one warning naming `STEPCLIMB_STORAGE_URL` when storage is not configured (B1a).
pub async fn open_storage(config: &StorageConfig) -> Result<Option<Storage>, StartupError> {
    let Some(storage) = Storage::open(config)? else {
        warn_storage_not_configured();
        return Ok(None);
    };
    storage.probe().await?;
    tracing::info!(storage = %config, "object storage passed the startup probe");
    Ok(Some(storage))
}

/// The one warning `serve` logs when storage is not configured (B1a).
fn warn_storage_not_configured() {
    tracing::warn!(
        "object storage is not configured: set {} to store course files; until then uploads \
         and file downloads answer 503 storage.not_configured",
        STORAGE_URL.var
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

/// Connect, [`prepare`] the database, then run what `config.role` asks for until SIGINT or
/// SIGTERM (spec m1/01 B13 to B16): the whole HTTP application (`all`, `web`) or only the probes
/// (`worker`) on `config.bind`, and the job worker (`all`, `worker`) on a pool of its own. On the
/// signal the listener stops taking connections and the worker stops claiming; `serve` returns
/// once both have finished, the worker after at most [`stepclimb_jobs::SHUTDOWN_GRACE`].
pub async fn serve(config: Config) -> Result<(), StartupError> {
    log_configuration(&config);
    warn_about_development_keys(&config.encryption_keys);
    warn_about_insecure_providers(&config);
    let storage = open_storage(&config.storage).await?;
    let pool = connect(&config.database_url).await?;
    let clock = Clock::system();
    prepare(&pool, &clock).await?;
    let config_secrets = config_providers::apply(
        &pool,
        &config.providers,
        config.allow_insecure_providers,
        clock.now(),
    )
    .await?;
    // Built before the listener exists, so `/readyz` never answers before it (B16).
    let worker = if config.role.runs_jobs() {
        Some(job_worker(&config, clock.clone()).await?)
    } else {
        None
    };
    let listener = listen(&config).await?;
    let stop = Stop::on_signal();
    let app: fn(AppState) -> Router = if config.role.serves_app() {
        router
    } else {
        probes_router
    };
    let opened = Opened {
        config_secrets,
        storage,
    };
    let http = serve_on(listener, config, opened, pool, clock, app, stop.wait());
    let jobs = async {
        if let Some(jobs) = worker {
            jobs.run(&stop).await;
        }
        Ok(())
    };
    tokio::try_join!(http, jobs).map(|_| ())
}

/// Bind `config.bind` and log the address the listener got, which names the port when `bind`
/// asks for port 0.
async fn listen(config: &Config) -> Result<TcpListener, StartupError> {
    let addr = config.bind;
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|source| StartupError::Bind { addr, source })?;
    let bound = listener
        .local_addr()
        .map_err(|source| StartupError::LocalAddr { addr, source })?;
    tracing::info!(addr = %bound, role = %config.role, "listening");
    Ok(listener)
}

/// What an `all` or `worker` process runs besides HTTP: the job worker, the scheduler of
/// periodic jobs and the outbox dispatcher (spec m1/01 B14, B17, B21), on the job pool
/// ([`connect_jobs`]).
struct JobRunner {
    worker: Worker,
    options: RunOptions,
    pool: PgPool,
    registry: Registry,
    clock: Clock,
}

impl JobRunner {
    /// Run the worker, the scheduler and the dispatcher until `stop`, then let each finish
    /// (B15).
    async fn run(self, stop: &Stop) {
        let dispatcher = outbox::dispatch::run(
            self.pool.clone(),
            self.registry.clone(),
            self.clock.clone(),
            POLL_INTERVAL,
            stop.wait(),
        );
        let scheduler = schedule::run(
            self.pool,
            self.registry,
            self.clock,
            schedule::TICK,
            stop.wait(),
        );
        tokio::join!(
            self.worker.run(self.options, stop.wait()),
            scheduler,
            dispatcher
        );
    }
}

/// The job runner of an `all` or `worker` process, with this binary's registry, serving every
/// queue in it, its options from `config`, and the schedule rows of its periodic kinds written
/// (spec m1/01 B17).
async fn job_worker(config: &Config, clock: Clock) -> Result<JobRunner, StartupError> {
    let pool = connect_jobs(&config.database_url, config.worker_concurrency).await?;
    let registry = jobs::registry().map_err(StartupError::Registry)?;
    schedule::sync(&pool, &registry, clock.now())
        .await
        .map_err(StartupError::Schedules)?;
    outbox::warn_unknown_topics(&pool, &registry)
        .await
        .map_err(StartupError::UnknownTopics)?;
    let queues = registry.queues();
    tracing::info!(
        concurrency = %config.worker_concurrency,
        queues = ?queues,
        periodic = ?registry.periodic().iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        "job worker started"
    );
    let worker = Worker::new(
        pool.clone(),
        registry.clone(),
        clock.clone(),
        Random::system(),
        queues,
    );
    let options = RunOptions::new(config.worker_concurrency);
    Ok(JobRunner {
        worker,
        options,
        pool,
        registry,
        clock,
    })
}

/// The process's stop request: set once by SIGINT or SIGTERM, read by the listener and the job
/// worker alike.
#[derive(Clone)]
struct Stop(watch::Receiver<bool>);

impl Stop {
    /// Start listening for SIGINT and SIGTERM.
    fn on_signal() -> Self {
        let (tx, rx) = watch::channel(false);
        tokio::spawn(async move {
            shutdown_signal().await;
            let _ = tx.send(true);
        });
        Self(rx)
    }

    /// Completes once the stop is requested. A closed channel, which only an ended signal task
    /// leaves, counts as a stop.
    fn wait(&self) -> impl Future<Output = ()> + Send + 'static {
        let mut rx = self.0.clone();
        async move {
            let _ = rx.wait_for(|stopped| *stopped).await;
        }
    }
}

/// What [`serve`] made from the configuration before it listens.
#[derive(Debug, Default)]
pub struct Opened {
    /// The secrets of the providers `stepclimb.toml` declares.
    pub config_secrets: ConfigSecrets,
    /// The probed store, or `None` when storage is not configured.
    pub storage: Option<Storage>,
}

/// Build the application state from `config`, what [`serve`] `opened`, `db` and `clock`, with
/// the rate limiter `config` sets, and serve `app(state)` on `listener` until `shutdown`
/// completes. Each request carries its TCP peer's address (`ConnectInfo<SocketAddr>`), which the
/// rate limits start from. [`serve`] passes [`router`]; a test passes a router with a
/// rate-limited route.
pub async fn serve_on(
    listener: TcpListener,
    config: Config,
    opened: Opened,
    db: PgPool,
    clock: Clock,
    app: impl FnOnce(AppState) -> Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), StartupError> {
    let rate_limits = RateLimiter::new(config.limits(), config.trusted_proxies.clone());
    let providers =
        Providers::new(config.allow_insecure_providers)?.with_config_secrets(opened.config_secrets);
    let state = AppState {
        db,
        clock,
        keys: config.encryption_keys,
        public_url: config.public_url,
        allowed_origins: config.allowed_origins,
        rate_limits,
        providers,
        passwords: PasswordHasher::default(),
        storage: opened.storage,
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
        assert!(
            output.contains("anyone can sign app access tokens this server accepts with it"),
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
    fn the_job_pool_has_a_connection_per_job_and_one_each_for_claims_ticks_and_dispatch() {
        for (concurrency, max) in [(1, 4), (4, 7), (64, 67)] {
            let concurrency = std::num::NonZeroU32::new(concurrency).unwrap();
            let options = jobs_pool_options(concurrency);
            assert_eq!(options.get_max_connections(), max, "{concurrency}");
            assert_eq!(options.get_acquire_timeout(), ACQUIRE_TIMEOUT);
        }
        assert_eq!(POOL_MAX_CONNECTIONS, 10, "the HTTP pool keeps its size");
    }

    #[test]
    fn a_ring_without_the_development_key_logs_nothing() {
        let output = warnings_for("prod:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=");
        assert_eq!(output, "");
    }

    #[test]
    fn a_branding_fallback_is_warned_about_naming_the_organization_and_each_failing_pair() {
        let rendered =
            stepclimb_core::theme::render(&stepclimb_core::theme::Brand::stepclimb_default())
                .unwrap();
        let mut reports = rendered.reports().to_vec();
        let (fg, bg) = {
            let pair = &mut reports[1].pairs[9];
            pair.pass = false;
            pair.ratio = 4.123;
            (pair.foreground.clone(), pair.background.clone())
        };
        let org_id = Uuid::nil();
        let error = RenderError::Contrast(reports);
        assert_eq!(
            failed_pairs(&error),
            [format!("Dark {fg} on {bg} 4.12 < 4.5")]
        );
        let output = captured(|| warn_about_branding_fallback(org_id, &failed_pairs(&error)));
        assert!(output.contains("WARN"), "{output}");
        assert!(output.contains(&org_id.to_string()), "{output}");
        assert!(
            output.contains(&format!("Dark {fg} on {bg} 4.12 < 4.5")),
            "{output}"
        );
        assert_eq!(output.lines().count(), 1, "{output}");
        let no_step = RenderError::NoStep("brand");
        assert_eq!(
            failed_pairs(&no_step),
            ["generator invariant: no scale step fits brand"]
        );
        let RenderError::Contrast(reports) = error else {
            unreachable!()
        };
        let stored = Fallback { report: reports };
        assert_eq!(
            stored_pairs(&stored),
            [format!("Dark {fg} on {bg} 4.12 < 4.5")]
        );
        let refused = Fallback { report: Vec::new() };
        assert_eq!(stored_pairs(&refused), [REFUSED]);
    }
}
