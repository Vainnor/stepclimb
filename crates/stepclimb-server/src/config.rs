// SPDX-License-Identifier: AGPL-3.0-or-later
//! Configuration from `STEPCLIMB_*` environment variables.
//!
//! The plan (Technical -> Hosting and operations) puts `STEPCLIMB_*` variables over an optional
//! `stepclimb.toml`, with `*_FILE` variants for secrets, and bad configuration stops startup with
//! a clear error. This module reads the variables and the `_FILE` variant of the database URL;
//! the TOML file comes with the Compose deploy spec (`docs/specs/m0/14-compose-deploy.md`).

use std::net::SocketAddr;

/// The address `stepclimb serve` listens on when `STEPCLIMB_BIND` is not set.
pub const DEFAULT_BIND: &str = "127.0.0.1:8080";

/// Server configuration, validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// `STEPCLIMB_BIND`: the socket address to listen on.
    pub bind: SocketAddr,
    /// `STEPCLIMB_DATABASE_URL`, or the contents of the file `STEPCLIMB_DATABASE_URL_FILE` names.
    pub database_url: String,
}

/// Why the configuration was refused. Each message names the variable to fix.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error(
        "STEPCLIMB_DATABASE_URL is not set (or set STEPCLIMB_DATABASE_URL_FILE to a file holding it)"
    )]
    MissingDatabaseUrl,
    #[error("set only one of STEPCLIMB_DATABASE_URL and STEPCLIMB_DATABASE_URL_FILE")]
    DatabaseUrlTwice,
    #[error("STEPCLIMB_DATABASE_URL_FILE names {path}, which could not be read: {reason}")]
    DatabaseUrlFile { path: String, reason: String },
    #[error("STEPCLIMB_DATABASE_URL must start with postgres:// or postgresql://")]
    DatabaseUrlScheme,
    #[error("STEPCLIMB_BIND is {value:?}, which is not a socket address such as 0.0.0.0:8080")]
    Bind { value: String },
}

impl Config {
    /// Read the configuration from the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Read the configuration through `lookup`, which returns a variable's value or `None`.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let bind_value = lookup("STEPCLIMB_BIND").unwrap_or_else(|| DEFAULT_BIND.to_owned());
        let bind = bind_value
            .parse::<SocketAddr>()
            .map_err(|_| ConfigError::Bind {
                value: bind_value.clone(),
            })?;
        let database_url = database_url(&lookup)?;
        Ok(Self { bind, database_url })
    }
}

fn database_url(lookup: &impl Fn(&str) -> Option<String>) -> Result<String, ConfigError> {
    let direct = lookup("STEPCLIMB_DATABASE_URL").filter(|v| !v.is_empty());
    let file = lookup("STEPCLIMB_DATABASE_URL_FILE").filter(|v| !v.is_empty());
    let url = match (direct, file) {
        (Some(_), Some(_)) => return Err(ConfigError::DatabaseUrlTwice),
        (Some(url), None) => url,
        (None, Some(path)) => std::fs::read_to_string(&path)
            .map(|contents| contents.trim().to_owned())
            .map_err(|e| ConfigError::DatabaseUrlFile {
                path,
                reason: e.to_string(),
            })?,
        (None, None) => return Err(ConfigError::MissingDatabaseUrl),
    };
    if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        Ok(url)
    } else {
        Err(ConfigError::DatabaseUrlScheme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn reads_the_database_url_and_defaults_the_bind_address() {
        let config =
            Config::from_lookup(env(&[("STEPCLIMB_DATABASE_URL", "postgres://u@h/db")])).unwrap();
        assert_eq!(config.database_url, "postgres://u@h/db");
        assert_eq!(config.bind, DEFAULT_BIND.parse::<SocketAddr>().unwrap());
    }

    #[test]
    fn reads_the_bind_address() {
        let config = Config::from_lookup(env(&[
            ("STEPCLIMB_DATABASE_URL", "postgresql://u@h/db"),
            ("STEPCLIMB_BIND", "0.0.0.0:9000"),
        ]))
        .unwrap();
        assert_eq!(config.bind, "0.0.0.0:9000".parse::<SocketAddr>().unwrap());
    }

    #[test]
    fn a_missing_database_url_is_refused_by_name() {
        let error = Config::from_lookup(env(&[])).unwrap_err();
        assert_eq!(error, ConfigError::MissingDatabaseUrl);
        assert!(error.to_string().contains("STEPCLIMB_DATABASE_URL"));
    }

    #[test]
    fn an_empty_database_url_counts_as_missing() {
        let error = Config::from_lookup(env(&[("STEPCLIMB_DATABASE_URL", "")])).unwrap_err();
        assert_eq!(error, ConfigError::MissingDatabaseUrl);
    }

    #[test]
    fn a_bad_bind_address_is_refused_with_its_value() {
        let error = Config::from_lookup(env(&[
            ("STEPCLIMB_DATABASE_URL", "postgres://u@h/db"),
            ("STEPCLIMB_BIND", "localhost"),
        ]))
        .unwrap_err();
        assert_eq!(
            error,
            ConfigError::Bind {
                value: "localhost".to_owned()
            }
        );
    }

    #[test]
    fn a_non_postgres_url_is_refused() {
        let error =
            Config::from_lookup(env(&[("STEPCLIMB_DATABASE_URL", "mysql://u@h/db")])).unwrap_err();
        assert_eq!(error, ConfigError::DatabaseUrlScheme);
    }

    #[test]
    fn the_file_variant_is_read_and_trimmed() {
        let dir = std::env::temp_dir().join(format!("stepclimb-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("database_url");
        std::fs::write(&path, "postgres://u@h/db\n").unwrap();
        let path_str = path.to_str().unwrap().to_owned();

        let config =
            Config::from_lookup(env(&[("STEPCLIMB_DATABASE_URL_FILE", &path_str)])).unwrap();

        assert_eq!(config.database_url, "postgres://u@h/db");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unreadable_file_variant_names_the_path() {
        let error = Config::from_lookup(env(&[(
            "STEPCLIMB_DATABASE_URL_FILE",
            "/nonexistent/stepclimb/database_url",
        )]))
        .unwrap_err();
        assert!(
            matches!(error, ConfigError::DatabaseUrlFile { ref path, .. }
            if path == "/nonexistent/stepclimb/database_url")
        );
    }

    #[test]
    fn both_variants_at_once_are_refused() {
        let error = Config::from_lookup(env(&[
            ("STEPCLIMB_DATABASE_URL", "postgres://u@h/db"),
            ("STEPCLIMB_DATABASE_URL_FILE", "/run/secrets/db"),
        ]))
        .unwrap_err();
        assert_eq!(error, ConfigError::DatabaseUrlTwice);
    }
}
