// SPDX-License-Identifier: AGPL-3.0-or-later
//! `[[providers]]` in `stepclimb.toml` (docs/specs/m0/06-oauth2-and-vatsim-preset.md B11 to B13,
//! Q2; 05-oidc.md Q2 (c)).
//!
//! Each entry is one table holding the provider's own members, and beside them the members of
//! its kind's config (`stepclimb_auth::oidc_config`, `stepclimb_auth::oauth2_config`), checked
//! by the same validation the admin API uses:
//!
//! | Member | Rule | When absent |
//! | --- | --- | --- |
//! | `kind` | `"oidc"` or `"oauth2"` | refused |
//! | `key` | 1 to 32 lowercase letters, digits and hyphens, starting with a letter or digit (Q5); not `local` | refused |
//! | `label` | a non-empty string of at most 200 characters without control characters | refused |
//! | `enabled` | a boolean | `true` |
//! | `org` | the slug of the organization the provider belongs to; required on an instance with more than one | the instance's one organization |
//! | `client_secret`, `client_secret_file` | exactly one: the secret, or a file whose trimmed contents are the secret, relative to the directory holding `stepclimb.toml` | refused |
//!
//! The secret is never written to the database: [`ConfigProvider::stored`] builds a config with
//! `source: "config"` and no secret, and the server keeps the secret in memory. No error repeats
//! a value or a `_file` path, and the secret leaves this module only as a [`ClientSecret`].
//!
//! Loading checks every entry under the URL policy of a single-organization instance, since
//! there is no database yet; `stepclimb serve` checks again under the instance's real policy
//! once it knows how many organizations there are (`crate::config_providers`).

use std::collections::BTreeMap;
use std::fmt;

use serde_json::{Map, Value};
use stepclimb_auth::oauth2_config::{self, Oauth2Config};
use stepclimb_auth::oidc_config::{self, OidcConfig};
use stepclimb_auth::outbound::UrlPolicy;
use stepclimb_auth::provider_config::{ClientSecret, ConfigErrors, ConfigProblem, code};
use stepclimb_core::identity_providers::{self, Kind};

use super::ConfigFile;

/// The longest `label`, in characters.
pub const LABEL_MAX_CHARS: usize = 200;

/// The members of an entry that are the provider's own, not its kind's config.
const OWN_MEMBERS: [&str; 7] = [
    "kind",
    "key",
    "label",
    "enabled",
    "org",
    "client_secret",
    "client_secret_file",
];

/// The client secret's stand-in when the kind's validation runs: the real secret was checked
/// already and never enters the config JSON.
const SECRET_STAND_IN: &str = "stepclimb.toml";

/// One `[[providers]]` entry, checked. `Debug` never shows the secret.
#[derive(Clone)]
pub struct ConfigProvider {
    /// The entry's 1-based position in the file.
    pub entry: usize,
    pub key: String,
    /// [`Kind::Oidc`] or [`Kind::Oauth2`].
    pub kind: Kind,
    pub label: String,
    pub enabled: bool,
    /// The organization's slug, when the entry names one.
    pub org: Option<String>,
    /// The kind's config members as JSON, without the secret.
    config: Map<String, Value>,
    secret: ClientSecret,
}

impl fmt::Debug for ConfigProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfigProvider")
            .field("entry", &self.entry)
            .field("key", &self.key)
            .field("kind", &self.kind)
            .field("enabled", &self.enabled)
            .field("org", &self.org)
            .finish_non_exhaustive()
    }
}

impl ConfigProvider {
    /// The secret read from the file.
    pub fn secret(&self) -> &ClientSecret {
        &self.secret
    }

    /// The row's stored `config` under `policy`: the kind's config with `source: "config"` and
    /// no secret, plus `label`. The problems are the kind's validation's, as messages naming
    /// the TOML member.
    pub fn stored(&self, policy: UrlPolicy) -> Result<Map<String, Value>, Vec<String>> {
        let mut input = self.config.clone();
        input.insert("client_secret".into(), SECRET_STAND_IN.into());
        let input = Value::Object(input);
        let mut stored = match self.kind {
            Kind::Oauth2 => oauth2_config::validate(&input, policy, None)
                .map(|draft| Oauth2Config::from_config_file(draft.settings).to_json()),
            // `ConfigProvider::parse` sets only the two outside kinds.
            Kind::Oidc | Kind::Local => oidc_config::validate(&input, policy, None)
                .map(|draft| OidcConfig::from_config_file(draft.settings).to_json()),
        }
        .map_err(|ConfigErrors(problems)| {
            problems.iter().map(problem_message).collect::<Vec<_>>()
        })?;
        stored.insert("label".into(), self.label.as_str().into());
        Ok(stored)
    }
}

/// Why `[[providers]]` was refused: the entry, its key when it has a readable one, and every
/// problem found in it, each naming a member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryError {
    pub entry: usize,
    pub key: Option<String>,
    pub problems: Vec<String>,
}

impl fmt::Display for EntryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[[providers]] entry {}", self.entry)?;
        if let Some(key) = &self.key {
            write!(f, " (key {key:?})")?;
        }
        write!(f, ": {}", self.problems.join("; "))
    }
}

/// Every `[[providers]]` entry of `value` (the `providers` key), checked under the policy of a
/// single-organization instance with the insecure switch `allow_insecure`. `file` resolves a
/// relative `client_secret_file`. An error is the first entry with a problem, or a key two
/// entries for the same organization share.
pub(super) fn parse(
    value: &toml::Value,
    file: &ConfigFile,
    allow_insecure: bool,
) -> Result<Vec<ConfigProvider>, ProvidersError> {
    let toml::Value::Array(entries) = value else {
        return Err(ProvidersError::NotArray);
    };
    let policy = UrlPolicy::new(allow_insecure, true);
    let mut providers = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let provider = parse_entry(index + 1, entry, file)?;
        provider.stored(policy).map_err(|problems| EntryError {
            entry: provider.entry,
            key: Some(provider.key.clone()),
            problems,
        })?;
        providers.push(provider);
    }
    check_unique_keys(&providers)?;
    Ok(providers)
}

/// Why `providers` was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvidersError {
    NotArray,
    Entry(EntryError),
}

impl From<EntryError> for ProvidersError {
    fn from(error: EntryError) -> Self {
        Self::Entry(error)
    }
}

/// Two entries with the same `org` (both absent, or the same slug) and `key` are refused.
fn check_unique_keys(providers: &[ConfigProvider]) -> Result<(), EntryError> {
    let mut seen: BTreeMap<(Option<&str>, &str), usize> = BTreeMap::new();
    for provider in providers {
        let id = (provider.org.as_deref(), provider.key.as_str());
        if let Some(first) = seen.insert(id, provider.entry) {
            return Err(EntryError {
                entry: provider.entry,
                key: Some(provider.key.clone()),
                problems: vec![format!(
                    "`key` is also the key of entry {first} for the same organization"
                )],
            });
        }
    }
    Ok(())
}

/// The entry's own members and its kind's config, collecting every problem.
fn parse_entry(
    entry: usize,
    value: &toml::Value,
    file: &ConfigFile,
) -> Result<ConfigProvider, EntryError> {
    let toml::Value::Table(table) = value else {
        return Err(EntryError {
            entry,
            key: None,
            problems: vec!["is not a table".to_owned()],
        });
    };
    let mut problems = Vec::new();
    let key = table.get("key").and_then(toml::Value::as_str);
    let fail = |problems: Vec<String>| EntryError {
        entry,
        key: key.map(str::to_owned),
        problems,
    };
    let kind = kind(table.get("kind"), &mut problems);
    let key = valid_key(table.get("key"), &mut problems);
    let label = label(table.get("label"), &mut problems);
    let enabled = flag(table.get("enabled"), "enabled", true, &mut problems);
    let org = org(table.get("org"), &mut problems);
    let secret = secret(table, file, &mut problems);
    let config = kind_config(table, &mut problems);
    if !problems.is_empty() {
        return Err(fail(problems));
    }
    match (kind, key, label, enabled, org, secret, config) {
        (
            Some(kind),
            Some(key),
            Some(label),
            Some(enabled),
            Some(org),
            Some(secret),
            Some(config),
        ) => Ok(ConfigProvider {
            entry,
            key,
            kind,
            label,
            enabled,
            org,
            config,
            secret,
        }),
        // Each step that returns `None` adds a problem.
        _ => Err(fail(vec!["could not be read".to_owned()])),
    }
}

fn kind(value: Option<&toml::Value>, problems: &mut Vec<String>) -> Option<Kind> {
    match value.map(toml::Value::as_str) {
        None => problems.push("`kind` is required (\"oidc\" or \"oauth2\")".to_owned()),
        Some(Some("oidc")) => return Some(Kind::Oidc),
        Some(Some("oauth2")) => return Some(Kind::Oauth2),
        Some(_) => problems.push("`kind` must be \"oidc\" or \"oauth2\"".to_owned()),
    }
    None
}

fn valid_key(value: Option<&toml::Value>, problems: &mut Vec<String>) -> Option<String> {
    match value.map(toml::Value::as_str) {
        None => problems.push("`key` is required".to_owned()),
        Some(Some("local")) => {
            problems.push("`key` cannot be \"local\", the organization's own accounts".to_owned());
        }
        Some(Some(key)) if identity_providers::valid_key(key) => return Some(key.to_owned()),
        Some(_) => problems.push(
            "`key` must be 1 to 32 lowercase letters, digits and hyphens, starting with a letter \
             or digit"
                .to_owned(),
        ),
    }
    None
}

fn label(value: Option<&toml::Value>, problems: &mut Vec<String>) -> Option<String> {
    match value.map(toml::Value::as_str) {
        None => problems.push("`label` is required".to_owned()),
        Some(Some(label))
            if !label.is_empty()
                && label.chars().count() <= LABEL_MAX_CHARS
                && !label.chars().any(char::is_control) =>
        {
            return Some(label.to_owned());
        }
        Some(_) => problems.push(format!(
            "`label` must be a string of 1 to {LABEL_MAX_CHARS} characters without control \
             characters"
        )),
    }
    None
}

fn flag(
    value: Option<&toml::Value>,
    name: &str,
    default: bool,
    problems: &mut Vec<String>,
) -> Option<bool> {
    match value {
        None => Some(default),
        Some(toml::Value::Boolean(on)) => Some(*on),
        Some(_) => {
            problems.push(format!("`{name}` must be true or false"));
            None
        }
    }
}

fn org(value: Option<&toml::Value>, problems: &mut Vec<String>) -> Option<Option<String>> {
    match value.map(toml::Value::as_str) {
        None => Some(None),
        Some(Some(slug)) if !slug.is_empty() => Some(Some(slug.to_owned())),
        Some(_) => {
            problems.push("`org` must be an organization's slug".to_owned());
            None
        }
    }
}

/// `client_secret`, or the trimmed contents of the file `client_secret_file` names; exactly one.
/// No problem repeats the secret or the path.
fn secret(
    table: &toml::Table,
    file: &ConfigFile,
    problems: &mut Vec<String>,
) -> Option<ClientSecret> {
    let text = match (table.get("client_secret"), table.get("client_secret_file")) {
        (Some(_), Some(_)) => {
            problems.push("set only one of `client_secret` and `client_secret_file`".to_owned());
            return None;
        }
        (None, None) => {
            problems.push("`client_secret` or `client_secret_file` is required".to_owned());
            return None;
        }
        (Some(toml::Value::String(secret)), None) => secret.clone(),
        (Some(_), None) => {
            problems.push("`client_secret` must be a string in quotes".to_owned());
            return None;
        }
        (None, Some(toml::Value::String(path))) if !path.is_empty() => {
            match std::fs::read_to_string(file.resolve(path)) {
                Ok(contents) => contents.trim().to_owned(),
                Err(error) => {
                    problems.push(format!(
                        "`client_secret_file` names a file that could not be read: {}",
                        error.kind()
                    ));
                    return None;
                }
            }
        }
        (None, Some(_)) => {
            problems.push("`client_secret_file` must be a path in quotes".to_owned());
            return None;
        }
    };
    let secret = ClientSecret::from_config_file(text);
    if secret.is_none() {
        problems.push("the client secret is empty".to_owned());
    }
    secret
}

/// The members that are not the entry's own, as the JSON the kind's validation reads.
fn kind_config(table: &toml::Table, problems: &mut Vec<String>) -> Option<Map<String, Value>> {
    let before = problems.len();
    let mut config = Map::new();
    for (name, value) in table {
        if OWN_MEMBERS.contains(&name.as_str()) {
            continue;
        }
        if let Some(json) = to_json(value, name, problems) {
            config.insert(name.clone(), json);
        }
    }
    (problems.len() == before).then_some(config)
}

/// A TOML value as JSON. A date or time has no JSON form, and a config member never takes one.
fn to_json(value: &toml::Value, path: &str, problems: &mut Vec<String>) -> Option<Value> {
    Some(match value {
        toml::Value::String(s) => Value::String(s.clone()),
        toml::Value::Integer(n) => Value::from(*n),
        toml::Value::Float(n) => {
            serde_json::Number::from_f64(*n).map_or(Value::Null, Value::Number)
        }
        toml::Value::Boolean(b) => Value::Bool(*b),
        toml::Value::Datetime(_) => {
            problems.push(format!("`{path}` has the wrong type"));
            return None;
        }
        toml::Value::Array(items) => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, item)| to_json(item, &format!("{path}.{i}"), problems))
                .collect::<Option<_>>()?,
        ),
        toml::Value::Table(table) => Value::Object(
            table
                .iter()
                .map(|(name, item)| {
                    to_json(item, &format!("{path}.{name}"), problems).map(|v| (name.clone(), v))
                })
                .collect::<Option<_>>()?,
        ),
    })
}

/// A problem of the kind's validation as a message naming the TOML member: the JSON Pointer's
/// segments joined by `.`, as TOML writes nested keys.
fn problem_message(problem: &ConfigProblem) -> String {
    let member = problem
        .pointer
        .split('/')
        .skip(1)
        .map(|segment| segment.replace("~1", "/").replace("~0", "~"))
        .collect::<Vec<_>>()
        .join(".");
    let what = match problem.code {
        code::FIELD_REQUIRED => "is required",
        code::FIELD_UNKNOWN => "is not a member of this kind of provider",
        code::FIELD_TYPE => "has the wrong type",
        code::FIELD_EMPTY => "must not be empty",
        code::URL_INVALID => {
            "must be an absolute https URL without a fragment, credentials or blanks (an OIDC \
             issuer also takes no query)"
        }
        code::URL_PARAMETER_RESERVED => "sets a query parameter the sign-in request sets itself",
        code::URL_REFUSED => {
            "is refused by the outbound URL policy (http and loopback addresses need \
             allow_insecure_providers; private addresses need an instance with one organization)"
        }
        code::SCOPES_MISSING_OPENID => "must include \"openid\"",
        code::SCOPE_INVALID => "is not an OAuth 2 scope token",
        code::POINTER_INVALID => "is not a JSON Pointer naming a member (it starts with /)",
        code::TEXT_CONTAINS_NUL => "contains the NUL character",
        other => other,
    };
    format!("`{member}` {what}")
}

#[cfg(test)]
mod tests;
