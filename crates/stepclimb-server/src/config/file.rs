// SPDX-License-Identifier: AGPL-3.0-or-later
//! `stepclimb.toml`: flat top-level keys, one per setting, with a `_file` key beside each secret.
//!
//! Each value is kept as an untyped [`toml::Value`] with its span, so a wrong type is reported
//! without the value in the message, and every error names the line. The whole file is checked
//! when it is parsed, including keys a variable overrides. A relative `_file` path resolves
//! against the directory holding `stepclimb.toml`.
//!
//! The plan's `[[providers]]` is a record, not a setting: `FileKeys` keeps it as one untyped
//! value too, since serde's own type errors quote the value they refused, which could print a
//! secret, and `super::providers` checks it member by member.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml::Spanned;

use super::{ConfigError, Origin, SETTINGS};

/// The keys `stepclimb.toml` may hold. An unknown key is a parse error naming its line. No
/// `Debug`: the values may be secrets; [`ConfigFile`]'s `Debug` lists the keys present.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileKeys {
    bind: Option<Spanned<toml::Value>>,
    log: Option<Spanned<toml::Value>>,
    database_url: Option<Spanned<toml::Value>>,
    database_url_file: Option<Spanned<toml::Value>>,
    encryption_keys: Option<Spanned<toml::Value>>,
    encryption_keys_file: Option<Spanned<toml::Value>>,
    public_url: Option<Spanned<toml::Value>>,
    allowed_origins: Option<Spanned<toml::Value>>,
    trusted_proxies: Option<Spanned<toml::Value>>,
    sign_in_rate_limit: Option<Spanned<toml::Value>>,
    token_rate_limit: Option<Spanned<toml::Value>>,
    failed_sign_in_limit: Option<Spanned<toml::Value>>,
    allow_insecure_providers: Option<Spanned<toml::Value>>,
    providers: Option<Spanned<toml::Value>>,
}

impl FileKeys {
    fn get(&self, key: &str) -> Option<&Spanned<toml::Value>> {
        match key {
            "bind" => self.bind.as_ref(),
            "log" => self.log.as_ref(),
            "database_url" => self.database_url.as_ref(),
            "database_url_file" => self.database_url_file.as_ref(),
            "encryption_keys" => self.encryption_keys.as_ref(),
            "encryption_keys_file" => self.encryption_keys_file.as_ref(),
            "public_url" => self.public_url.as_ref(),
            "allowed_origins" => self.allowed_origins.as_ref(),
            "trusted_proxies" => self.trusted_proxies.as_ref(),
            "sign_in_rate_limit" => self.sign_in_rate_limit.as_ref(),
            "token_rate_limit" => self.token_rate_limit.as_ref(),
            "failed_sign_in_limit" => self.failed_sign_in_limit.as_ref(),
            "allow_insecure_providers" => self.allow_insecure_providers.as_ref(),
            _ => None,
        }
    }
}

/// Every key `FileKeys` holds, a secret's `_file` key after its value.
fn all_keys() -> Vec<String> {
    let mut keys = Vec::new();
    for setting in SETTINGS {
        keys.push(setting.key.to_owned());
        if setting.secret {
            keys.push(format!("{}_file", setting.key));
        }
    }
    keys
}

/// A parsed and checked `stepclimb.toml`, not yet resolved against the environment. `Debug`
/// shows the path and the names of the keys present, never a value.
pub struct ConfigFile {
    path: PathBuf,
    text: String,
    keys: FileKeys,
}

impl ConfigFile {
    /// Read and parse the file at `path`.
    pub fn read(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|e| ConfigError::Unreadable {
            path: path.to_path_buf(),
            reason: e.kind().to_string(),
        })?;
        Self::parse(path.to_path_buf(), text)
    }

    /// Parse `text` as the file at `path`. The error carries the parser's description and the
    /// line, never the source text, which may hold a secret.
    pub fn parse(path: PathBuf, text: String) -> Result<Self, ConfigError> {
        let file = match toml::from_str::<FileKeys>(&text) {
            Ok(keys) => Self { path, text, keys },
            Err(error) => {
                return Err(ConfigError::Toml {
                    line: error.span().map(|span| line_of(&text, span.start)),
                    message: error.message().to_owned(),
                    path,
                });
            }
        };
        file.check()?;
        Ok(file)
    }

    /// Every value is a string, and no secret has both its value and its `_file`, whether or not
    /// a variable overrides the setting.
    fn check(&self) -> Result<(), ConfigError> {
        for setting in SETTINGS.iter().filter(|s| s.secret) {
            let file_key = format!("{}_file", setting.key);
            if let (Some((_, value)), Some((_, file))) =
                (self.string(setting.key)?, self.string(&file_key)?)
            {
                return Err(ConfigError::Twice { value, file });
            }
        }
        for key in all_keys() {
            self.string(&key)?;
        }
        Ok(())
    }

    /// The path a `_file` key names: relative paths resolve against this file's directory.
    pub(super) fn resolve(&self, path: &str) -> PathBuf {
        match self.path.parent() {
            Some(dir) => dir.join(path),
            None => PathBuf::from(path),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The `providers` value and where it starts, when the file has one.
    pub(super) fn providers(&self) -> Option<(&toml::Value, Origin)> {
        self.keys.providers.as_ref().map(|spanned| {
            let origin = Origin::Key {
                key: "providers".to_owned(),
                path: self.path.clone(),
                line: line_of(&self.text, spanned.span().start),
            };
            (spanned.get_ref(), origin)
        })
    }

    /// The string at `key` and where it was set, or `None` when the key is absent or empty.
    pub(super) fn string(&self, key: &str) -> Result<Option<(String, Origin)>, ConfigError> {
        let Some(spanned) = self.keys.get(key) else {
            return Ok(None);
        };
        let origin = Origin::Key {
            key: key.to_owned(),
            path: self.path.clone(),
            line: line_of(&self.text, spanned.span().start),
        };
        match spanned.get_ref() {
            toml::Value::String(value) if value.is_empty() => Ok(None),
            toml::Value::String(value) => Ok(Some((value.clone(), origin))),
            _ => Err(ConfigError::Invalid {
                origin,
                problem: "must be a string in quotes".to_owned(),
            }),
        }
    }
}

impl std::fmt::Debug for ConfigFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut present: Vec<String> = all_keys()
            .into_iter()
            .filter(|key| self.keys.get(key).is_some())
            .collect();
        if self.keys.providers.is_some() {
            present.push("providers".to_owned());
        }
        f.debug_struct("ConfigFile")
            .field("path", &self.path)
            .field("keys", &present)
            .finish_non_exhaustive()
    }
}

/// The 1-based line holding byte `offset` of `text`.
fn line_of(text: &str, offset: usize) -> usize {
    let end = offset.min(text.len());
    text.as_bytes()[..end]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}
