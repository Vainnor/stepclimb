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
//! A spec that adds configuration adds a [`Setting`] to [`SETTINGS`], its field to [`Config`] and
//! to `FileKeys` in `file.rs`, and its validation to [`Config::load`]
//! (`docs/specs/m0/14-compose-deploy.md` B11).

mod file;

use std::collections::BTreeMap;
use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use sqlx::postgres::PgConnectOptions;
use stepclimb_auth::encryption::KeyRing;
use tracing_subscriber::EnvFilter;

pub use file::ConfigFile;

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

/// Every setting [`Config`] reads.
pub const SETTINGS: &[Setting] = &[BIND, LOG, DATABASE_URL, ENCRYPTION_KEYS];

/// The variable naming the configuration file when `--config` is not given.
pub const CONFIG_VAR: &str = "STEPCLIMB_CONFIG";

/// The address `stepclimb serve` listens on when `STEPCLIMB_BIND` is not set.
pub const DEFAULT_BIND: &str = "127.0.0.1:8080";

/// The log filter when `STEPCLIMB_LOG` is not set.
pub const DEFAULT_LOG: &str = "info";

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
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("file", &self.file)
            .field("bind", &self.bind)
            .field("log", &self.log)
            .field("database_url", &redacted_database_url(&self.database_url))
            .field("encryption_keys", &self.encryption_keys)
            .finish()
    }
}

/// The parts of the database URL that are safe to print: user, host, port and database, read with
/// sqlx's own parser. The password and every query parameter (which can carry `password=`) are
/// left out.
fn redacted_database_url(url: &str) -> String {
    match url.parse::<PgConnectOptions>() {
        Ok(options) => format!(
            "postgres://{}@{}:{}/{} (password and parameters hidden)",
            options.get_username(),
            options.get_host(),
            options.get_port(),
            options.get_database().unwrap_or_default(),
        ),
        Err(_) => "<not a parseable PostgreSQL URL>".to_owned(),
    }
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
        Ok(Self {
            file: file.map(|f| f.path().to_path_buf()),
            bind,
            log,
            database_url,
            encryption_keys,
        })
    }
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

fn check_database_url(url: &str, origin: Origin) -> Result<(), ConfigError> {
    if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        Ok(())
    } else {
        Err(ConfigError::Invalid {
            origin,
            problem: format!(
                "the URL must start with postgres:// or postgresql://; for example {}",
                DATABASE_URL.example
            ),
        })
    }
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
