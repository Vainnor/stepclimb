// SPDX-License-Identifier: AGPL-3.0-or-later
//! Configuration from `STEPCLIMB_*` environment variables.
//!
//! The plan (Technical -> Hosting and operations) puts `STEPCLIMB_*` variables over an optional
//! `stepclimb.toml`, with `*_FILE` variants for secrets, and bad configuration stops startup with
//! a clear error. This module reads the variables and the `_FILE` variants of the database URL
//! and the encryption keys; the TOML file comes with the Compose deploy spec
//! (`docs/specs/m0/14-compose-deploy.md`).

use std::fmt;
use std::net::SocketAddr;

use sqlx::postgres::PgConnectOptions;
use stepclimb_auth::encryption::{KeyRing, KeyRingError};

/// The address `stepclimb serve` listens on when `STEPCLIMB_BIND` is not set.
pub const DEFAULT_BIND: &str = "127.0.0.1:8080";

/// Server configuration, validated. `Debug` hides the database password and query string and
/// shows only the encryption key ids.
#[derive(Clone)]
pub struct Config {
    /// `STEPCLIMB_BIND`: the socket address to listen on.
    pub bind: SocketAddr,
    /// `STEPCLIMB_DATABASE_URL`, or the contents of the file `STEPCLIMB_DATABASE_URL_FILE` names.
    pub database_url: String,
    /// `STEPCLIMB_ENCRYPTION_KEYS`, or the contents of the file `STEPCLIMB_ENCRYPTION_KEYS_FILE`
    /// names: the AES-256-GCM key ring (`stepclimb_auth::encryption`), required.
    pub encryption_keys: KeyRing,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("bind", &self.bind)
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

/// Why the configuration was refused. Each message names the variable to fix.
/// No message repeats a secret's value.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("{name} is not set (or set {name}_FILE to a file holding it)")]
    Missing { name: &'static str },
    #[error("set only one of {name} and {name}_FILE")]
    Twice { name: &'static str },
    /// The path is left out: an operator may have pasted the secret itself into `{name}_FILE`.
    #[error("{name}_FILE names a file that could not be read: {reason}")]
    File { name: &'static str, reason: String },
    #[error("STEPCLIMB_DATABASE_URL must start with postgres:// or postgresql://")]
    DatabaseUrlScheme,
    #[error("STEPCLIMB_BIND is {value:?}, which is not a socket address such as 0.0.0.0:8080")]
    Bind { value: String },
    #[error("STEPCLIMB_ENCRYPTION_KEYS {0}")]
    EncryptionKeys(KeyRingError),
}

const DATABASE_URL: &str = "STEPCLIMB_DATABASE_URL";
const ENCRYPTION_KEYS: &str = "STEPCLIMB_ENCRYPTION_KEYS";

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
        let encryption_keys = KeyRing::parse(&secret(&lookup, ENCRYPTION_KEYS)?)
            .map_err(ConfigError::EncryptionKeys)?;
        Ok(Self {
            bind,
            database_url,
            encryption_keys,
        })
    }
}

/// A secret from the variable `name` or from the file `{name}_FILE` names (trimmed); exactly one
/// must be set. An empty value counts as unset.
fn secret(
    lookup: &impl Fn(&str) -> Option<String>,
    name: &'static str,
) -> Result<String, ConfigError> {
    let direct = lookup(name).filter(|v| !v.is_empty());
    let file = lookup(&format!("{name}_FILE")).filter(|v| !v.is_empty());
    match (direct, file) {
        (Some(_), Some(_)) => Err(ConfigError::Twice { name }),
        (Some(value), None) => Ok(value),
        (None, Some(path)) => std::fs::read_to_string(&path)
            .map(|contents| contents.trim().to_owned())
            .map_err(|e| ConfigError::File {
                name,
                reason: e.kind().to_string(),
            }),
        (None, None) => Err(ConfigError::Missing { name }),
    }
}

fn database_url(lookup: &impl Fn(&str) -> Option<String>) -> Result<String, ConfigError> {
    let url = secret(lookup, DATABASE_URL)?;
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

    const KEYS: &str = "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key| map.get(key).cloned()
    }

    /// `pairs` plus a valid encryption key ring, unless `pairs` sets one of its variables.
    fn with_keys(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let mut all = pairs.to_vec();
        if !pairs.iter().any(|(k, _)| k.starts_with(ENCRYPTION_KEYS)) {
            all.push((ENCRYPTION_KEYS, KEYS));
        }
        env(&all)
    }

    fn temp_file(name: &str, contents: &str) -> (std::path::PathBuf, String) {
        let dir =
            std::env::temp_dir().join(format!("stepclimb-config-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        let path_str = path.to_str().unwrap().to_owned();
        (dir, path_str)
    }

    #[test]
    fn reads_the_database_url_and_defaults_the_bind_address() {
        let config = Config::from_lookup(with_keys(&[(
            "STEPCLIMB_DATABASE_URL",
            "postgres://u@h/db",
        )]))
        .unwrap();
        assert_eq!(config.database_url, "postgres://u@h/db");
        assert_eq!(config.bind, DEFAULT_BIND.parse::<SocketAddr>().unwrap());
    }

    #[test]
    fn reads_the_bind_address() {
        let config = Config::from_lookup(with_keys(&[
            ("STEPCLIMB_DATABASE_URL", "postgresql://u@h/db"),
            ("STEPCLIMB_BIND", "0.0.0.0:9000"),
        ]))
        .unwrap();
        assert_eq!(config.bind, "0.0.0.0:9000".parse::<SocketAddr>().unwrap());
    }

    #[test]
    fn a_missing_database_url_is_refused_by_name() {
        let error = Config::from_lookup(with_keys(&[])).unwrap_err();
        assert_eq!(
            error,
            ConfigError::Missing {
                name: "STEPCLIMB_DATABASE_URL"
            }
        );
        assert_eq!(
            error.to_string(),
            "STEPCLIMB_DATABASE_URL is not set (or set STEPCLIMB_DATABASE_URL_FILE to a file holding it)"
        );
    }

    #[test]
    fn an_empty_database_url_counts_as_missing() {
        let error = Config::from_lookup(with_keys(&[("STEPCLIMB_DATABASE_URL", "")])).unwrap_err();
        assert_eq!(
            error,
            ConfigError::Missing {
                name: "STEPCLIMB_DATABASE_URL"
            }
        );
    }

    #[test]
    fn a_bad_bind_address_is_refused_with_its_value() {
        let error = Config::from_lookup(with_keys(&[
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
        let error = Config::from_lookup(with_keys(&[("STEPCLIMB_DATABASE_URL", "mysql://u@h/db")]))
            .unwrap_err();
        assert_eq!(error, ConfigError::DatabaseUrlScheme);
    }

    #[test]
    fn the_file_variant_is_read_and_trimmed() {
        let (dir, path) = temp_file("database_url", "postgres://u@h/db\n");
        let config =
            Config::from_lookup(with_keys(&[("STEPCLIMB_DATABASE_URL_FILE", &path)])).unwrap();
        assert_eq!(config.database_url, "postgres://u@h/db");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unreadable_file_variant_names_the_variable_only() {
        let error = Config::from_lookup(with_keys(&[(
            "STEPCLIMB_DATABASE_URL_FILE",
            "/nonexistent/stepclimb/database_url",
        )]))
        .unwrap_err();
        assert!(matches!(
            error,
            ConfigError::File {
                name: "STEPCLIMB_DATABASE_URL",
                ..
            }
        ));
        assert_eq!(
            error.to_string(),
            "STEPCLIMB_DATABASE_URL_FILE names a file that could not be read: entity not found"
        );
    }

    #[test]
    fn a_secret_pasted_into_a_file_variable_is_not_echoed() {
        let error = Config::from_lookup(env(&[
            ("STEPCLIMB_DATABASE_URL", "postgres://u@h/db"),
            (
                "STEPCLIMB_ENCRYPTION_KEYS_FILE",
                "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
            ),
        ]))
        .unwrap_err();
        let message = format!("{error} {error:?}");
        assert!(
            message.contains("STEPCLIMB_ENCRYPTION_KEYS_FILE"),
            "{message}"
        );
        assert!(!message.contains("AAEC"), "{message}");
    }

    #[test]
    fn both_variants_at_once_are_refused() {
        let error = Config::from_lookup(with_keys(&[
            ("STEPCLIMB_DATABASE_URL", "postgres://u@h/db"),
            ("STEPCLIMB_DATABASE_URL_FILE", "/run/secrets/db"),
        ]))
        .unwrap_err();
        assert_eq!(
            error,
            ConfigError::Twice {
                name: "STEPCLIMB_DATABASE_URL"
            }
        );
    }

    #[test]
    fn reads_the_encryption_key_ring() {
        let config = Config::from_lookup(env(&[
            ("STEPCLIMB_DATABASE_URL", "postgres://u@h/db"),
            (
                "STEPCLIMB_ENCRYPTION_KEYS",
                "k2:ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8=,k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
            ),
        ]))
        .unwrap();
        assert_eq!(config.encryption_keys.current_key_id(), "k2");
        let printed = format!("{config:?}");
        assert!(printed.contains(r#"key_ids: ["k2", "k1"]"#), "{printed}");
        assert!(
            !printed.contains("AAEC") && !printed.contains("ICEi"),
            "{printed}"
        );
    }

    #[test]
    fn missing_encryption_keys_stop_startup_by_name() {
        let error = Config::from_lookup(env(&[("STEPCLIMB_DATABASE_URL", "postgres://u@h/db")]))
            .unwrap_err();
        assert_eq!(
            error,
            ConfigError::Missing {
                name: "STEPCLIMB_ENCRYPTION_KEYS"
            }
        );
        assert!(error.to_string().contains("STEPCLIMB_ENCRYPTION_KEYS_FILE"));
    }

    #[test]
    fn the_encryption_keys_file_variant_is_read() {
        let (dir, path) = temp_file("keys", &format!("{KEYS}\n"));
        let config = Config::from_lookup(env(&[
            ("STEPCLIMB_DATABASE_URL", "postgres://u@h/db"),
            ("STEPCLIMB_ENCRYPTION_KEYS_FILE", &path),
        ]))
        .unwrap();
        assert_eq!(config.encryption_keys.current_key_id(), "k1");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_bad_key_is_refused_without_repeating_it() {
        let error = Config::from_lookup(with_keys(&[
            ("STEPCLIMB_DATABASE_URL", "postgres://u@h/db"),
            (
                "STEPCLIMB_ENCRYPTION_KEYS",
                "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYX",
            ),
        ]))
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            r#"STEPCLIMB_ENCRYPTION_KEYS entry 1 (id "k1") is not 32 bytes in standard padded base64"#
        );
    }

    #[test]
    fn both_encryption_key_variants_at_once_are_refused() {
        let error = Config::from_lookup(env(&[
            ("STEPCLIMB_DATABASE_URL", "postgres://u@h/db"),
            ("STEPCLIMB_ENCRYPTION_KEYS", KEYS),
            ("STEPCLIMB_ENCRYPTION_KEYS_FILE", "/run/secrets/keys"),
        ]))
        .unwrap_err();
        assert_eq!(
            error,
            ConfigError::Twice {
                name: "STEPCLIMB_ENCRYPTION_KEYS"
            }
        );
    }

    #[test]
    fn debug_hides_the_database_password_and_query() {
        let config = Config::from_lookup(with_keys(&[(
            "STEPCLIMB_DATABASE_URL",
            "postgres://app:s3cret-pa55@db.internal:5432/stepclimb?sslmode=require&password=q-s3cret",
        )]))
        .unwrap();
        let printed = format!("{config:?}");
        assert!(
            printed.contains(
                r#"database_url: "postgres://app@db.internal:5432/stepclimb (password and parameters hidden)""#
            ),
            "{printed}"
        );
        assert!(!printed.contains("s3cret"), "{printed}");
    }

    #[test]
    fn redaction_never_shows_a_password_with_odd_characters() {
        for url in [
            "postgres://u:zq%40ss%2Fw0rd@h/db",
            "postgres://u:zq?ss@h/db",
            "postgres://u:zq@ss@h/db",
            "postgres://u@h/db?password=zq",
            "not a url zq",
        ] {
            let printed = redacted_database_url(url);
            assert!(!printed.contains("zq"), "{url} -> {printed}");
        }
    }
}
