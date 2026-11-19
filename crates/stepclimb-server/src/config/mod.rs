// SPDX-License-Identifier: AGPL-3.0-or-later
//! Configuration: `STEPCLIMB_*` environment variables over an optional `stepclimb.toml`.
//!
//! Every setting has one variable and one top-level TOML key of the same name, `STEPCLIMB_BIND`
//! and `bind` (ADR 0022). The file is read only when `--config` or `STEPCLIMB_CONFIG` names it,
//! and a variable overrides the file setting by setting. A secret also has a `_FILE` variant in
//! both places (`STEPCLIMB_DATABASE_URL_FILE`, `database_url_file`) naming a file whose trimmed
//! contents are the value; a relative path in a variable is relative to the working directory,
//! and in the file to the file's directory. An empty value counts as unset. An unknown
//! `STEPCLIMB_*` variable or TOML key is an error, so a misspelled secret cannot start the server
//! without it. No error repeats a secret's value or a `_FILE` path, since an operator may have
//! pasted the secret itself there.
//!
//! Identity providers can also be declared in the file as `[[providers]]` tables, which have no
//! variable ([`providers`], spec 06 B11 to B13).
//!
//! A spec that adds configuration adds a [`Setting`] to [`SETTINGS`], its field to [`Config`] and
//! to `FileKeys` in `file.rs`, and its validation to [`Config::load`]
//! (`docs/specs/m0/14-compose-deploy.md` B11).

mod file;
pub mod providers;
mod storage;
mod transcode;

use std::collections::BTreeMap;
use std::fmt;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use sqlx::postgres::PgConnectOptions;
use stepclimb_auth::encryption::KeyRing;
use tracing_subscriber::EnvFilter;

pub use file::ConfigFile;
pub use providers::ConfigProvider;
pub use storage::{
    DEFAULT_REGION, S3Config, STORAGE_S3_ACCESS_KEY_ID, STORAGE_S3_ALLOW_HTTP, STORAGE_S3_ENDPOINT,
    STORAGE_S3_REGION, STORAGE_S3_SECRET_ACCESS_KEY, STORAGE_URL, StorageConfig,
};
pub use transcode::{
    DEFAULT_MAX_VIDEO_SECONDS, FFMPEG_PATH, FFPROBE_PATH, MAX_MAX_VIDEO_SECONDS,
    MAX_TRANSCODE_CONCURRENCY, MAX_TRANSCODE_THREADS, MAX_VIDEO_SECONDS, PRLIMIT_PATH,
    TRANSCODE_CONCURRENCY, TRANSCODE_DIR, TRANSCODE_THREADS, TranscodeConfig,
};

use crate::client_ip::TrustedProxies;
use crate::rate_limit::Limits;
use crate::web_origin::WebOrigin;

/// One configuration setting: its variable, its TOML key and whether its value is a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setting {
    /// The environment variable, `STEPCLIMB_` and the key in upper case.
    pub var: &'static str,
    /// The top-level key in `stepclimb.toml`.
    pub key: &'static str,
    /// A secret also has `{var}_FILE` and `{key}_file`, and never appears in logs or errors.
    pub secret: bool,
    /// A valid value, shown in errors.
    pub example: &'static str,
}

pub const BIND: Setting = Setting {
    var: "STEPCLIMB_BIND",
    key: "bind",
    secret: false,
    example: "0.0.0.0:8080",
};
pub const LOG: Setting = Setting {
    var: "STEPCLIMB_LOG",
    key: "log",
    secret: false,
    example: "info or stepclimb_server=debug,info",
};
pub const DATABASE_URL: Setting = Setting {
    var: "STEPCLIMB_DATABASE_URL",
    key: "database_url",
    secret: true,
    example: "postgres://stepclimb@db:5432/stepclimb",
};
pub const ENCRYPTION_KEYS: Setting = Setting {
    var: "STEPCLIMB_ENCRYPTION_KEYS",
    key: "encryption_keys",
    secret: true,
    example: "k1:<32 random bytes in base64, from openssl rand -base64 32>",
};

pub const PUBLIC_URL: Setting = Setting {
    var: "STEPCLIMB_PUBLIC_URL",
    key: "public_url",
    secret: false,
    example: "https://lms.example.org",
};
pub const ALLOWED_ORIGINS: Setting = Setting {
    var: "STEPCLIMB_ALLOWED_ORIGINS",
    key: "allowed_origins",
    secret: false,
    example: "http://localhost:5173 (the Vite dev server), comma-separated",
};
pub const TRUSTED_PROXIES: Setting = Setting {
    var: "STEPCLIMB_TRUSTED_PROXIES",
    key: "trusted_proxies",
    secret: false,
    example: "10.201.73.2 (Caddy in the production Compose stack), comma-separated addresses \
              or CIDR ranges",
};
pub const SIGN_IN_RATE_LIMIT: Setting = Setting {
    var: "STEPCLIMB_SIGN_IN_RATE_LIMIT",
    key: "sign_in_rate_limit",
    secret: false,
    example: "20 (sign-in requests per minute from one client address)",
};
pub const TOKEN_RATE_LIMIT: Setting = Setting {
    var: "STEPCLIMB_TOKEN_RATE_LIMIT",
    key: "token_rate_limit",
    secret: false,
    example: "60 (token and revoke requests per minute from one client address)",
};
pub const FAILED_SIGN_IN_LIMIT: Setting = Setting {
    var: "STEPCLIMB_FAILED_SIGN_IN_LIMIT",
    key: "failed_sign_in_limit",
    secret: false,
    example: "10 (failed password or TOTP checks per account per 15 minutes)",
};

pub const ALLOW_INSECURE_PROVIDERS: Setting = Setting {
    var: "STEPCLIMB_ALLOW_INSECURE_PROVIDERS",
    key: "allow_insecure_providers",
    secret: false,
    example: "true (only for tests and local development) or false",
};

pub const ROLE: Setting = Setting {
    var: "STEPCLIMB_ROLE",
    key: "role",
    secret: false,
    example: "all (HTTP and the job worker), web (HTTP only), worker (the job worker and the \
              probes only) or transcoder (the transcode queue and the probes only)",
};
pub const WORKER_CONCURRENCY: Setting = Setting {
    var: "STEPCLIMB_WORKER_CONCURRENCY",
    key: "worker_concurrency",
    secret: false,
    example: "4 (jobs one process runs at once, 1 to 64)",
};

pub const MAX_FILE_BYTES: Setting = Setting {
    var: "STEPCLIMB_MAX_FILE_BYTES",
    key: "max_file_bytes",
    secret: false,
    example: "104857600 (the largest file one upload may send, in bytes: 1 MiB to 5 GiB)",
};
pub const MAX_VIDEO_BYTES: Setting = Setting {
    var: "STEPCLIMB_MAX_VIDEO_BYTES",
    key: "max_video_bytes",
    secret: false,
    example: "5368709120 (the largest video one upload may send, in bytes: 1 MiB to 50 GiB)",
};

/// Every setting [`Config`] reads.
pub const SETTINGS: &[Setting] = &[
    BIND,
    LOG,
    DATABASE_URL,
    ENCRYPTION_KEYS,
    PUBLIC_URL,
    ALLOWED_ORIGINS,
    TRUSTED_PROXIES,
    SIGN_IN_RATE_LIMIT,
    TOKEN_RATE_LIMIT,
    FAILED_SIGN_IN_LIMIT,
    ALLOW_INSECURE_PROVIDERS,
    ROLE,
    WORKER_CONCURRENCY,
    MAX_FILE_BYTES,
    MAX_VIDEO_BYTES,
    MAX_VIDEO_SECONDS,
    TRANSCODE_CONCURRENCY,
    TRANSCODE_THREADS,
    TRANSCODE_DIR,
    FFMPEG_PATH,
    FFPROBE_PATH,
    PRLIMIT_PATH,
    STORAGE_URL,
    STORAGE_S3_ENDPOINT,
    STORAGE_S3_REGION,
    STORAGE_S3_ACCESS_KEY_ID,
    STORAGE_S3_SECRET_ACCESS_KEY,
    STORAGE_S3_ALLOW_HTTP,
];

/// The variable naming the configuration file when `--config` is not given.
pub const CONFIG_VAR: &str = "STEPCLIMB_CONFIG";

/// The address `stepclimb serve` listens on when `STEPCLIMB_BIND` is not set.
pub const DEFAULT_BIND: &str = "127.0.0.1:8080";

/// The log filter when `STEPCLIMB_LOG` is not set.
pub const DEFAULT_LOG: &str = "info";

/// The most jobs one process runs at once (`worker_concurrency`, spec m1/01 B14).
pub const MAX_WORKER_CONCURRENCY: u32 = 64;

/// `worker_concurrency` when it is not set.
pub const DEFAULT_WORKER_CONCURRENCY: NonZeroU32 = NonZeroU32::new(4).expect("4 is not zero");

/// The smallest `max_file_bytes`: 1 MiB (docs/specs/m1/06-files.md B4, Q2).
pub const MIN_MAX_FILE_BYTES: u64 = 1024 * 1024;

/// The largest `max_file_bytes`: 5 GiB, the most one S3 copy request takes (B7 copies an upload
/// to its file's key).
pub const MAX_MAX_FILE_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// `max_file_bytes` when it is not set: 100 MiB (Q2).
pub const DEFAULT_MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;

/// The smallest `max_video_bytes`: 1 MiB (docs/specs/m1/08-uploaded-video.md B4).
pub const MIN_MAX_VIDEO_BYTES: u64 = 1024 * 1024;

/// The largest `max_video_bytes`: 50 GiB (B4). Each chunk is stored as its own object, so no
/// object nears S3's 5 GiB copy limit.
pub const MAX_MAX_VIDEO_BYTES: u64 = 50 * 1024 * 1024 * 1024;

/// `max_video_bytes` when it is not set: 5 GiB (B4).
pub const DEFAULT_MAX_VIDEO_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// What a `stepclimb serve` process runs (`role`, `serve --role`; spec m1/01 B13, Q1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Role {
    /// The HTTP application and the job worker.
    #[default]
    All,
    /// The HTTP application only.
    Web,
    /// The job worker of every queue but `transcode`, with `/healthz` and `/readyz` the only
    /// HTTP routes.
    Worker,
    /// The job worker of the `transcode` queue only, with `/healthz` and `/readyz` the only HTTP
    /// routes (docs/specs/m1/08-uploaded-video.md B8).
    Transcoder,
}

impl Role {
    /// The spelling in configuration and on the command line.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Web => "web",
            Self::Worker => "worker",
            Self::Transcoder => "transcoder",
        }
    }

    /// Whether the process serves the whole HTTP application.
    #[must_use]
    pub fn serves_app(self) -> bool {
        matches!(self, Self::All | Self::Web)
    }

    /// Whether the process runs a job worker of any queue.
    #[must_use]
    pub fn runs_jobs(self) -> bool {
        !matches!(self, Self::Web)
    }

    /// Whether the process runs the worker of every queue but `transcode`, with the scheduler of
    /// periodic jobs and the outbox dispatcher.
    #[must_use]
    pub fn runs_general_jobs(self) -> bool {
        matches!(self, Self::All | Self::Worker)
    }

    /// Whether the process serves the `transcode` queue (docs/specs/m1/08-uploaded-video.md B8).
    #[must_use]
    pub fn runs_transcodes(self) -> bool {
        matches!(self, Self::All | Self::Transcoder)
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The `STEPCLIMB_*` environment variables, by name.
pub type Env = BTreeMap<String, String>;

/// Server configuration, validated. `Debug` hides the database password and query string and
/// shows only the encryption key ids.
#[derive(Clone)]
pub struct Config {
    /// The configuration file read, if any (`--config` or `STEPCLIMB_CONFIG`).
    pub file: Option<PathBuf>,
    /// `bind`: the socket address to listen on.
    pub bind: SocketAddr,
    /// `log`: the JSON log filter, in `tracing_subscriber::EnvFilter` syntax.
    pub log: String,
    /// `database_url`, secret.
    pub database_url: String,
    /// `encryption_keys`, secret: the AES-256-GCM key ring (`stepclimb_auth::encryption`),
    /// required.
    pub encryption_keys: KeyRing,
    /// `public_url`, required: the origin users open the instance at (spec 08 Q1). It is the
    /// first origin the CSRF check accepts in an `Origin` header, and an `https` one turns HSTS
    /// on.
    pub public_url: WebOrigin,
    /// `allowed_origins`: further origins an unsafe request's `Origin` header may name,
    /// comma-separated (spec 08 Q6). A request that sends no `Sec-Fetch-Site` is accepted only
    /// from these or the public URL, and one that sends `Origin` must name one of them even when
    /// it is same-origin (B14). Unset means none; development sets the Vite dev server's origin.
    pub allowed_origins: Vec<WebOrigin>,
    /// `trusted_proxies`: the addresses and CIDR ranges of the reverse proxies whose
    /// `X-Forwarded-For` names a request's client address for the rate limits (spec 08 Q7).
    /// Unset means none: every client is its TCP peer. A range must hold proxies only, never
    /// clients, and may be no wider than /8 (IPv4) or /32 (IPv6).
    pub trusted_proxies: TrustedProxies,
    /// `sign_in_rate_limit`: sign-in requests per minute per client address; 20 when unset.
    pub sign_in_rate_limit: NonZeroU32,
    /// `token_rate_limit`: `/oauth/token` and `/oauth/revoke` requests per minute per client
    /// address; 60 when unset.
    pub token_rate_limit: NonZeroU32,
    /// `failed_sign_in_limit`: failed password or TOTP checks per account per 15 minutes; 10
    /// when unset.
    pub failed_sign_in_limit: NonZeroU32,
    /// `allow_insecure_providers`: `true` lets the server fetch `http` and loopback URLs for
    /// identity providers and the admission webhook (spec 05 Q4,
    /// `stepclimb_auth::outbound::UrlPolicy`), which tests and the development stack need;
    /// `false` when unset. Never on in production: it lets an admin reach services on the
    /// server's own host.
    pub allow_insecure_providers: bool,
    /// `role`: what `stepclimb serve` runs; `all` when unset. `serve --role` overrides it.
    pub role: Role,
    /// `worker_concurrency`: the most jobs this process runs at once, 1 to
    /// [`MAX_WORKER_CONCURRENCY`]; [`DEFAULT_WORKER_CONCURRENCY`] when unset.
    pub worker_concurrency: NonZeroU32,
    /// `max_file_bytes`: the largest file one upload may send, in bytes, [`MIN_MAX_FILE_BYTES`]
    /// to [`MAX_MAX_FILE_BYTES`]; [`DEFAULT_MAX_FILE_BYTES`] when unset
    /// (docs/specs/m1/06-files.md B4, Q2).
    pub max_file_bytes: u64,
    /// `max_video_bytes`: the largest video one tus upload may declare, in bytes,
    /// [`MIN_MAX_VIDEO_BYTES`] to [`MAX_MAX_VIDEO_BYTES`]; [`DEFAULT_MAX_VIDEO_BYTES`] when unset
    /// (docs/specs/m1/08-uploaded-video.md B4).
    pub max_video_bytes: u64,
    /// The transcoder's settings (docs/specs/m1/08-uploaded-video.md B8, B9).
    pub transcode: TranscodeConfig,
    /// `[[providers]]`: the identity providers the file declares, checked, each with its
    /// client secret (spec 06 B11). `stepclimb serve` writes them to their organizations.
    pub providers: Vec<ConfigProvider>,
    /// `storage_url` and the `storage_s3_*` settings: where course files are stored, or
    /// [`StorageConfig::NotConfigured`] (docs/specs/m1/06-files.md B1, B1a). The secret access
    /// key is hidden from `Debug`.
    pub storage: StorageConfig,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("file", &self.file)
            .field("bind", &self.bind)
            .field("log", &self.log)
            .field("database_url", &redacted_database_url(&self.database_url))
            .field("encryption_keys", &self.encryption_keys)
            .field("public_url", &self.public_url)
            .field("allowed_origins", &self.allowed_origins)
            .field("trusted_proxies", &self.trusted_proxies)
            .field("sign_in_rate_limit", &self.sign_in_rate_limit)
            .field("token_rate_limit", &self.token_rate_limit)
            .field("failed_sign_in_limit", &self.failed_sign_in_limit)
            .field("allow_insecure_providers", &self.allow_insecure_providers)
            .field("role", &self.role)
            .field("worker_concurrency", &self.worker_concurrency)
            .field("max_file_bytes", &self.max_file_bytes)
            .field("max_video_bytes", &self.max_video_bytes)
            .field("transcode", &self.transcode)
            .field("providers", &self.providers)
            .field("storage", &self.storage)
            .finish()
    }
}

/// The parts of the database URL that are safe to print: user, host, port and database, read with
/// sqlx's own parser. The password and every query parameter (which can carry `password=`) are
/// left out, and so is a database name of other than letters, digits, `_` and `-`, since a
/// password with an unencoded `/` can end up there.
fn redacted_database_url(url: &str) -> String {
    match parse_database_url(url) {
        Ok(options) => format!(
            "postgres://{}@{}:{}/{} (password and parameters hidden)",
            options.get_username(),
            options.get_host(),
            options.get_port(),
            printable_database(options.get_database().unwrap_or_default()),
        ),
        Err(_) => "<not a parseable PostgreSQL URL>".to_owned(),
    }
}

fn printable_database(name: &str) -> &str {
    let plain = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if plain { name } else { "<hidden>" }
}

/// The query parameters sqlx 0.9 reads from a PostgreSQL URL (`PgConnectOptions::parse_from_url`),
/// plus `options[<name>]`. sqlx logs any other parameter with its value and ignores it, so the
/// loader refuses it instead; `database_url_parameters_sqlx_accepts` keeps this list honest.
const DATABASE_URL_PARAMETERS: &[&str] = &[
    "sslmode",
    "ssl-mode",
    "sslrootcert",
    "ssl-root-cert",
    "ssl-ca",
    "sslcert",
    "ssl-cert",
    "sslkey",
    "ssl-key",
    "statement-cache-capacity",
    "host",
    "hostaddr",
    "port",
    "dbname",
    "user",
    "password",
    "application_name",
    "options",
];

/// Why a database URL was refused. No variant carries any part of the URL: a parameter's name
/// can be the tail of a password with an unencoded `&` or `?`, so it is reported by position.
#[derive(Debug, PartialEq, Eq)]
enum DatabaseUrlProblem {
    Unparsable,
    /// The database part holds an `@`: a password with an unencoded `/` or `#` before it.
    AtInPath,
    /// The 1-based position of a query parameter sqlx does not read.
    UnknownParameter(usize),
}

/// Parse `url` as sqlx will, after checking that sqlx will not log any of its parameters.
fn parse_database_url(url: &str) -> Result<PgConnectOptions, DatabaseUrlProblem> {
    let parsed = url::Url::parse(url).map_err(|_| DatabaseUrlProblem::Unparsable)?;
    if parsed.path().contains('@') {
        return Err(DatabaseUrlProblem::AtInPath);
    }
    for (position, (name, _)) in parsed.query_pairs().enumerate() {
        let known = DATABASE_URL_PARAMETERS.contains(&&*name)
            || (name.starts_with("options[") && name.ends_with(']'));
        if !known {
            return Err(DatabaseUrlProblem::UnknownParameter(position + 1));
        }
    }
    url.parse().map_err(|_| DatabaseUrlProblem::Unparsable)
}

/// Where a value was set: a variable, or a key in the configuration file and its line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Var(String),
    Key {
        key: String,
        path: PathBuf,
        line: usize,
    },
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Var(name) => f.write_str(name),
            Self::Key { key, path, line } => write!(f, "`{key}` ({} line {line})", path.display()),
        }
    }
}

/// Why the configuration was refused. Each message names the variable or the TOML key (with its
/// file and line) to fix. No message repeats a secret's value or a `_FILE` path.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error(
        "{} is not set (set it, {}_FILE to a file holding it, or `{}` in the configuration \
         file); for example {}",
        .0.var, .0.var, .0.key, .0.example
    )]
    Missing(Setting),
    #[error("set only one of {value} and {file}")]
    Twice { value: Origin, file: Origin },
    #[error("{origin} names a file that could not be read: {reason}")]
    File { origin: Origin, reason: String },
    #[error("{origin}: {problem}")]
    Invalid { origin: Origin, problem: String },
    #[error(
        "{name} is not a Stepclimb setting; check its spelling (the variables are {})",
        known_vars().join(", ")
    )]
    UnknownVar { name: String },
    #[error("{name} is not valid UTF-8")]
    NotUnicode { name: String },
    #[error("could not read the configuration file {}: {reason}", path.display())]
    Unreadable { path: PathBuf, reason: String },
    #[error("{}: {error}", path.display())]
    Provider {
        path: PathBuf,
        error: providers::EntryError,
    },
    #[error("{}{}: {message}", path.display(), line.map(|l| format!(" line {l}")).unwrap_or_default())]
    Toml {
        path: PathBuf,
        line: Option<usize>,
        message: String,
    },
}

/// Every variable the server reads: each setting, each secret's `_FILE`, and `STEPCLIMB_CONFIG`.
pub fn known_vars() -> Vec<String> {
    let mut names = Vec::new();
    for setting in SETTINGS {
        names.push(setting.var.to_owned());
        if setting.secret {
            names.push(format!("{}_FILE", setting.var));
        }
    }
    names.push(CONFIG_VAR.to_owned());
    names
}

impl Config {
    /// Read the configuration from the process environment and the file `flag` names, or else
    /// the file `STEPCLIMB_CONFIG` names.
    pub fn from_env(flag: Option<&Path>) -> Result<Self, ConfigError> {
        let env = process_env()?;
        let file = config_path(flag, &env)
            .map(|path| ConfigFile::read(&path))
            .transpose()?;
        Self::load(&env, file.as_ref())
    }

    /// Validate the configuration given by `env` (the `STEPCLIMB_*` variables) over `file`.
    pub fn load(env: &Env, file: Option<&ConfigFile>) -> Result<Self, ConfigError> {
        if let Some(name) = env.keys().find(|name| !known_vars().contains(name)) {
            return Err(ConfigError::UnknownVar { name: name.clone() });
        }
        let sources = Sources { env, file };
        let bind = match sources.plain(&BIND)? {
            Some((value, origin)) => value.parse().map_err(|_| ConfigError::Invalid {
                origin,
                problem: format!(
                    "{value:?} is not a socket address; for example {}",
                    BIND.example
                ),
            })?,
            // DEFAULT_BIND is a literal socket address; a test parses it.
            None => DEFAULT_BIND
                .parse()
                .expect("DEFAULT_BIND is a socket address"),
        };
        let log = match sources.plain(&LOG)? {
            Some((value, origin)) => log_filter(value, origin)?,
            None => DEFAULT_LOG.to_owned(),
        };
        let (database_url, origin) = sources.secret(&DATABASE_URL)?;
        check_database_url(&database_url, origin)?;
        let (keys, origin) = sources.secret(&ENCRYPTION_KEYS)?;
        let encryption_keys = KeyRing::parse(&keys).map_err(|e| ConfigError::Invalid {
            origin,
            problem: format!("{e}; for example {}", ENCRYPTION_KEYS.example),
        })?;
        let public_url = match sources.plain(&PUBLIC_URL)? {
            Some((value, origin)) => web_origin(&value, origin, &PUBLIC_URL)?,
            None => return Err(ConfigError::Missing(PUBLIC_URL)),
        };
        let allowed_origins = match sources.plain(&ALLOWED_ORIGINS)? {
            Some((value, origin)) => allowed_origins(&value, origin)?,
            None => Vec::new(),
        };
        let trusted_proxies = match sources.plain(&TRUSTED_PROXIES)? {
            Some((value, origin)) => trusted_proxies(&value, origin)?,
            None => TrustedProxies::none(),
        };
        let allow_insecure_providers = sources.flag(&ALLOW_INSECURE_PROVIDERS)?;
        let role = match sources.plain(&ROLE)? {
            Some((value, origin)) => role(&value, origin)?,
            None => Role::default(),
        };
        let worker_concurrency = worker_concurrency(&sources)?;
        let max_file_bytes = max_file_bytes(&sources)?;
        let max_video_bytes = max_video_bytes(&sources)?;
        let transcode = transcode::load(&sources)?;
        let providers = match file {
            Some(file) => file_providers(file, allow_insecure_providers)?,
            None => Vec::new(),
        };
        let storage = storage::load(&sources)?;
        let defaults = Limits::DEFAULT;
        Ok(Self {
            file: file.map(|f| f.path().to_path_buf()),
            bind,
            log,
            database_url,
            encryption_keys,
            public_url,
            allowed_origins,
            trusted_proxies,
            sign_in_rate_limit: sources.count(&SIGN_IN_RATE_LIMIT, defaults.sign_in)?,
            token_rate_limit: sources.count(&TOKEN_RATE_LIMIT, defaults.token)?,
            failed_sign_in_limit: sources.count(&FAILED_SIGN_IN_LIMIT, defaults.failed_checks)?,
            allow_insecure_providers,
            role,
            worker_concurrency,
            max_file_bytes,
            max_video_bytes,
            transcode,
            providers,
            storage,
        })
    }

    /// The rate limits these settings configure.
    pub fn limits(&self) -> Limits {
        Limits {
            sign_in: self.sign_in_rate_limit,
            token: self.token_rate_limit,
            failed_checks: self.failed_sign_in_limit,
        }
    }
}

impl Config {
    /// Each non-secret setting's key and value, in [`SETTINGS`] order, but for the storage
    /// settings, which [`StorageConfig::report`] lists. `stepclimb check-config` prints these;
    /// the secret settings never appear.
    pub fn non_secret_values(&self) -> Vec<(&'static str, String)> {
        let mut values = vec![
            (BIND.key, self.bind.to_string()),
            (LOG.key, self.log.clone()),
            (PUBLIC_URL.key, self.public_url.to_string()),
            (ALLOWED_ORIGINS.key, self.allowed_origins_or_none()),
            (TRUSTED_PROXIES.key, self.trusted_proxies_or_none()),
            (SIGN_IN_RATE_LIMIT.key, self.sign_in_rate_limit.to_string()),
            (TOKEN_RATE_LIMIT.key, self.token_rate_limit.to_string()),
            (
                FAILED_SIGN_IN_LIMIT.key,
                self.failed_sign_in_limit.to_string(),
            ),
            (
                ALLOW_INSECURE_PROVIDERS.key,
                self.allow_insecure_providers.to_string(),
            ),
            (ROLE.key, self.role.to_string()),
            (WORKER_CONCURRENCY.key, self.worker_concurrency.to_string()),
            (MAX_FILE_BYTES.key, self.max_file_bytes.to_string()),
            (MAX_VIDEO_BYTES.key, self.max_video_bytes.to_string()),
        ];
        values.extend(self.transcode.report());
        values
    }

    /// The declared providers as `key (kind)`, `, disabled` added for one that is off, and
    /// `org <slug>` for one that names its organization, comma-separated; `(none)` when there
    /// are none. Never a secret.
    pub fn providers_or_none(&self) -> String {
        if self.providers.is_empty() {
            return "(none)".to_owned();
        }
        self.providers
            .iter()
            .map(|p| {
                let mut notes = vec![p.kind.as_str().to_owned()];
                if let Some(org) = &p.org {
                    notes.push(format!("org {org}"));
                }
                if !p.enabled {
                    notes.push("disabled".to_owned());
                }
                format!("{} ({})", p.key, notes.join(", "))
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// `trusted_proxies` as it is configured, comma-separated, or `(none)`.
    pub fn trusted_proxies_or_none(&self) -> String {
        if self.trusted_proxies.is_empty() {
            "(none)".to_owned()
        } else {
            self.trusted_proxies.to_string()
        }
    }

    fn allowed_origins_or_none(&self) -> String {
        if self.allowed_origins.is_empty() {
            "(none)".to_owned()
        } else {
            self.allowed_origins_list()
        }
    }

    /// `allowed_origins` as it is configured, comma-separated; empty when unset.
    pub fn allowed_origins_list(&self) -> String {
        self.allowed_origins
            .iter()
            .map(WebOrigin::as_str)
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// The `[[providers]]` of `file`, checked with the insecure switch `allow_insecure`.
fn file_providers(
    file: &ConfigFile,
    allow_insecure: bool,
) -> Result<Vec<ConfigProvider>, ConfigError> {
    let Some((value, origin)) = file.providers() else {
        return Ok(Vec::new());
    };
    providers::parse(value, file, allow_insecure).map_err(|error| match error {
        providers::ProvidersError::NotArray => ConfigError::Invalid {
            origin,
            problem: "must be an array of tables, each written [[providers]]".to_owned(),
        },
        providers::ProvidersError::Entry(error) => ConfigError::Provider {
            path: file.path().to_path_buf(),
            error,
        },
    })
}

/// The `STEPCLIMB_*` variables of this process. Other variables are not read.
fn process_env() -> Result<Env, ConfigError> {
    let mut env = Env::new();
    for (name, value) in std::env::vars_os() {
        let lossy = name.to_string_lossy();
        if !lossy.starts_with("STEPCLIMB_") {
            continue;
        }
        let (Some(name), Some(value)) = (name.to_str(), value.to_str()) else {
            return Err(ConfigError::NotUnicode {
                name: lossy.into_owned(),
            });
        };
        env.insert(name.to_owned(), value.to_owned());
    }
    Ok(env)
}

/// The configuration file to read: `--config` over `STEPCLIMB_CONFIG`; none when neither is set.
fn config_path(flag: Option<&Path>, env: &Env) -> Option<PathBuf> {
    flag.map(Path::to_path_buf).or_else(|| {
        env.get(CONFIG_VAR)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
    })
}

fn log_filter(value: String, origin: Origin) -> Result<String, ConfigError> {
    match EnvFilter::builder().parse(&value) {
        Ok(_) => Ok(value),
        Err(_) => Err(ConfigError::Invalid {
            origin,
            problem: format!("{value:?} is not a log filter; for example {}", LOG.example),
        }),
    }
}

/// The error names the setting and the problem but never repeats the value, which can carry a
/// password (`https://user:password@host`).
fn web_origin(value: &str, origin: Origin, setting: &Setting) -> Result<WebOrigin, ConfigError> {
    WebOrigin::parse(value).map_err(|problem| ConfigError::Invalid {
        origin,
        problem: format!("{problem}; for example {}", setting.example),
    })
}

fn role(value: &str, origin: Origin) -> Result<Role, ConfigError> {
    <Role as clap::ValueEnum>::from_str(value, false).map_err(|_| ConfigError::Invalid {
        origin,
        problem: format!("{value:?} is not a role; for example {}", ROLE.example),
    })
}

/// `worker_concurrency`: [`Sources::count`] (a whole number of 1 or more, digits only), then at
/// most [`MAX_WORKER_CONCURRENCY`]; [`DEFAULT_WORKER_CONCURRENCY`] when neither source sets it
/// (spec m1/01 B14).
fn worker_concurrency(sources: &Sources<'_>) -> Result<NonZeroU32, ConfigError> {
    let count = sources.count(&WORKER_CONCURRENCY, DEFAULT_WORKER_CONCURRENCY)?;
    if count.get() <= MAX_WORKER_CONCURRENCY {
        return Ok(count);
    }
    // `count` only returns a value above the default when a source set it.
    let origin = match sources.plain(&WORKER_CONCURRENCY)? {
        Some((_, origin)) => origin,
        None => Origin::Var(WORKER_CONCURRENCY.var.to_owned()),
    };
    Err(ConfigError::Invalid {
        origin,
        problem: format!(
            "{count} is more than {MAX_WORKER_CONCURRENCY}; for example {}",
            WORKER_CONCURRENCY.example
        ),
    })
}

/// `max_file_bytes`: a whole number of bytes, digits only, from [`MIN_MAX_FILE_BYTES`] to
/// [`MAX_MAX_FILE_BYTES`]; [`DEFAULT_MAX_FILE_BYTES`] when neither source sets it
/// (docs/specs/m1/06-files.md B4, Q2).
fn max_file_bytes(sources: &Sources<'_>) -> Result<u64, ConfigError> {
    let range = (MIN_MAX_FILE_BYTES, "1 MiB", MAX_MAX_FILE_BYTES, "5 GiB");
    byte_count(sources, &MAX_FILE_BYTES, range).map(|n| n.unwrap_or(DEFAULT_MAX_FILE_BYTES))
}

/// `max_video_bytes`: as [`max_file_bytes`], from [`MIN_MAX_VIDEO_BYTES`] to
/// [`MAX_MAX_VIDEO_BYTES`]; [`DEFAULT_MAX_VIDEO_BYTES`] when neither source sets it
/// (docs/specs/m1/08-uploaded-video.md B4).
fn max_video_bytes(sources: &Sources<'_>) -> Result<u64, ConfigError> {
    let range = (MIN_MAX_VIDEO_BYTES, "1 MiB", MAX_MAX_VIDEO_BYTES, "50 GiB");
    byte_count(sources, &MAX_VIDEO_BYTES, range).map(|n| n.unwrap_or(DEFAULT_MAX_VIDEO_BYTES))
}

/// The byte count `setting` names, digits only, within `(min, min_label, max, max_label)`;
/// `None` when neither source sets it.
fn byte_count(
    sources: &Sources<'_>,
    setting: &Setting,
    (min, min_label, max, max_label): (u64, &str, u64, &str),
) -> Result<Option<u64>, ConfigError> {
    let Some((value, origin)) = sources.plain(setting)? else {
        return Ok(None);
    };
    let digits = !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit());
    let problem = match value.parse::<u64>() {
        Ok(bytes) if digits && (min..=max).contains(&bytes) => return Ok(Some(bytes)),
        Ok(bytes) if digits => {
            format!("{bytes} is outside {min} ({min_label}) to {max} ({max_label})")
        }
        _ => format!("{value:?} is not a whole number of bytes"),
    };
    Err(ConfigError::Invalid {
        origin,
        problem: format!("{problem}; for example {}", setting.example),
    })
}

/// A comma-separated list of origins; blanks around an entry are ignored, an empty entry is not.
/// An error names the entry by position only, as [`web_origin`] does.
fn allowed_origins(value: &str, origin: Origin) -> Result<Vec<WebOrigin>, ConfigError> {
    value
        .split(',')
        .enumerate()
        .map(|(index, entry)| {
            let entry = entry.trim();
            WebOrigin::parse(entry).map_err(|problem| ConfigError::Invalid {
                origin: origin.clone(),
                problem: format!(
                    "entry {} {problem}; for example {}",
                    index + 1,
                    ALLOWED_ORIGINS.example
                ),
            })
        })
        .collect()
}

/// A comma-separated list of proxy addresses and ranges, refused by entry position.
fn trusted_proxies(value: &str, origin: Origin) -> Result<TrustedProxies, ConfigError> {
    TrustedProxies::parse(value).map_err(|(position, problem)| ConfigError::Invalid {
        origin,
        problem: format!(
            "entry {position} {problem}; for example {}",
            TRUSTED_PROXIES.example
        ),
    })
}

fn check_database_url(url: &str, origin: Origin) -> Result<(), ConfigError> {
    let problem = if !(url.starts_with("postgres://") || url.starts_with("postgresql://")) {
        "the URL must start with postgres:// or postgresql://".to_owned()
    } else {
        match parse_database_url(url) {
            Ok(_) => return Ok(()),
            Err(DatabaseUrlProblem::Unparsable) => "is not a valid PostgreSQL URL".to_owned(),
            Err(DatabaseUrlProblem::AtInPath) => {
                "has an @ after the host; percent-encode @, /, ?, & and # in the password \
                 (@ as %40)"
                    .to_owned()
            }
            Err(DatabaseUrlProblem::UnknownParameter(position)) => format!(
                "query parameter {position} of the URL is not one the server reads (it reads {}; \
                 percent-encode & and ? in the password)",
                DATABASE_URL_PARAMETERS.join(", ")
            ),
        }
    };
    Err(ConfigError::Invalid {
        origin,
        problem: format!("{problem}; for example {}", DATABASE_URL.example),
    })
}

/// The variables and the file, resolved setting by setting: a variable wins over the file, and
/// an empty value counts as unset.
struct Sources<'a> {
    env: &'a Env,
    file: Option<&'a ConfigFile>,
}

impl Sources<'_> {
    fn var(&self, name: &str) -> Option<(String, Origin)> {
        self.env
            .get(name)
            .filter(|value| !value.is_empty())
            .map(|value| (value.clone(), Origin::Var(name.to_owned())))
    }

    fn key(&self, key: &str) -> Result<Option<(String, Origin)>, ConfigError> {
        match self.file {
            Some(file) => file.string(key),
            None => Ok(None),
        }
    }

    /// The file a `_FILE` variable or `_file` key names. A relative path in a variable is
    /// relative to the working directory; in the configuration file, to that file's directory.
    fn path_of(&self, path: &str, origin: &Origin) -> PathBuf {
        match (origin, self.file) {
            (Origin::Key { .. }, Some(file)) => file.resolve(path),
            _ => PathBuf::from(path),
        }
    }

    /// A setting's value and where it was set, or `None` when neither source sets it.
    fn plain(&self, setting: &Setting) -> Result<Option<(String, Origin)>, ConfigError> {
        match self.var(setting.var) {
            Some(found) => Ok(Some(found)),
            None => self.key(setting.key),
        }
    }

    /// A whole number of 1 or more, or `default` when neither source sets it.
    /// Digits only: `NonZeroU32::from_str` also takes a leading `+`.
    fn count(&self, setting: &Setting, default: NonZeroU32) -> Result<NonZeroU32, ConfigError> {
        let Some((value, origin)) = self.plain(setting)? else {
            return Ok(default);
        };
        let digits = value.bytes().all(|b| b.is_ascii_digit());
        match value.parse() {
            Ok(count) if digits => Ok(count),
            _ => Err(ConfigError::Invalid {
                origin,
                problem: format!(
                    "{value:?} is not a whole number of 1 or more; for example {}",
                    setting.example
                ),
            }),
        }
    }

    /// `true` or `false`, or `false` when neither source sets it.
    fn flag(&self, setting: &Setting) -> Result<bool, ConfigError> {
        match self.plain(setting)? {
            None => Ok(false),
            Some((value, _)) if value == "true" => Ok(true),
            Some((value, _)) if value == "false" => Ok(false),
            Some((value, origin)) => Err(ConfigError::Invalid {
                origin,
                problem: format!(
                    "{value:?} is not true or false; for example {}",
                    setting.example
                ),
            }),
        }
    }

    /// Where `setting` is set, in any of its forms (the variable, the key, and a secret's
    /// `_FILE` and `_file`), or `None` when it is not set at all. The value is not read.
    fn origin_if_set(&self, setting: &Setting) -> Result<Option<Origin>, ConfigError> {
        let mut forms = vec![(setting.var.to_owned(), setting.key.to_owned())];
        if setting.secret {
            forms.push((
                format!("{}_FILE", setting.var),
                format!("{}_file", setting.key),
            ));
        }
        for (var, key) in forms {
            if let Some((_, origin)) = self.var(&var) {
                return Ok(Some(origin));
            }
            if let Some((_, origin)) = self.key(&key)? {
                return Ok(Some(origin));
            }
        }
        Ok(None)
    }

    /// A secret from the variable or its `_FILE`, else from the key or its `_file`; exactly one
    /// of each pair may be set. A value read from a file is trimmed.
    fn secret(&self, setting: &Setting) -> Result<(String, Origin), ConfigError> {
        let mut pair = (
            self.var(setting.var),
            self.var(&format!("{}_FILE", setting.var)),
        );
        if pair.0.is_none() && pair.1.is_none() {
            pair = (
                self.key(setting.key)?,
                self.key(&format!("{}_file", setting.key))?,
            );
        }
        match pair {
            (Some((_, value)), Some((_, file))) => Err(ConfigError::Twice { value, file }),
            (Some(found), None) => Ok(found),
            (None, Some((path, origin))) => {
                match std::fs::read_to_string(self.path_of(&path, &origin)) {
                    Ok(contents) => Ok((contents.trim().to_owned(), origin)),
                    Err(e) => Err(ConfigError::File {
                        origin,
                        reason: e.kind().to_string(),
                    }),
                }
            }
            (None, None) => Err(ConfigError::Missing(*setting)),
        }
    }
}

#[cfg(test)]
mod tests;
