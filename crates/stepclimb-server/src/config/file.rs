// SPDX-License-Identifier: AGPL-3.0-or-later
//! `stepclimb.toml`: flat top-level keys, one per setting, with a `_file` key beside each secret.
//!
//! Each value is kept as an untyped [`toml::Value`] with its span, so a wrong type is reported by
//! [`ConfigFile::string`] without the value in the message, and every error names the line. An
//! array of tables such as the plan's `[[providers]]` is a typed field of [`FileKeys`].

use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml::Spanned;

use super::{ConfigError, Origin};

/// The keys `stepclimb.toml` may hold. An unknown key is a parse error naming its line.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileKeys {
    bind: Option<Spanned<toml::Value>>,
    log: Option<Spanned<toml::Value>>,
    database_url: Option<Spanned<toml::Value>>,
    database_url_file: Option<Spanned<toml::Value>>,
    encryption_keys: Option<Spanned<toml::Value>>,
    encryption_keys_file: Option<Spanned<toml::Value>>,
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
            _ => None,
        }
    }
}

/// A parsed `stepclimb.toml`, not yet resolved against the environment.
#[derive(Debug)]
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
        match toml::from_str::<FileKeys>(&text) {
            Ok(keys) => Ok(Self { path, text, keys }),
            Err(error) => Err(ConfigError::Toml {
                line: error.span().map(|span| line_of(&text, span.start)),
                message: error.message().to_owned(),
                path,
            }),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
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

/// The 1-based line holding byte `offset` of `text`.
fn line_of(text: &str, offset: usize) -> usize {
    let end = offset.min(text.len());
    text.as_bytes()[..end]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}
