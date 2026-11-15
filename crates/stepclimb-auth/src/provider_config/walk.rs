// SPDX-License-Identifier: AGPL-3.0-or-later
//! The member rules both config kinds apply, collecting every problem in a [`Walk`].

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value};
use url::Url;

use super::{
    ClientSecret, ConfigErrors, ConfigProblem, DraftSecret, SOURCE, SOURCE_CONFIG, SealedSecret,
    StoredConfigError, StoredSecret, code,
};
use crate::json_pointer::{JsonPointer, pointer_to};
use crate::outbound::UrlPolicy;

/// The problems found so far.
#[derive(Default)]
pub(crate) struct Walk {
    problems: Vec<ConfigProblem>,
}

impl Walk {
    pub(crate) fn fail(&mut self, path: &[&str], code: &'static str) {
        self.problems.push(ConfigProblem {
            pointer: pointer_to(path),
            code,
        });
    }

    /// The number of problems so far, to tell whether a step added one.
    pub(crate) fn count(&self) -> usize {
        self.problems.len()
    }

    /// `value` when no rule was broken, every problem otherwise.
    pub(crate) fn finish<T>(self, value: Option<T>) -> Result<T, ConfigErrors> {
        match value {
            Some(value) if self.problems.is_empty() => Ok(value),
            _ => Err(ConfigErrors(self.problems)),
        }
    }

    /// As [`Walk::finish`], for a stored config: the first problem only.
    pub(crate) fn finish_stored<T>(self, value: Option<T>) -> Result<T, StoredConfigError> {
        self.finish(value).map_err(|ConfigErrors(problems)| {
            let (pointer, code) = problems.into_iter().next().map_or_else(
                || (String::new(), code::FIELD_TYPE),
                |p| (p.pointer, p.code),
            );
            StoredConfigError::Invalid { pointer, code }
        })
    }
}

/// `config` as an object, or a problem at the root.
pub(crate) fn object<'a>(walk: &mut Walk, config: &'a Value) -> Option<&'a Map<String, Value>> {
    let map = config.as_object();
    if map.is_none() {
        walk.fail(&[], code::FIELD_TYPE);
    }
    map
}

/// A problem for every member of `map` (at `path`) not in `known`, so a misspelled member is
/// never silently ignored.
pub(crate) fn unknown_members(
    walk: &mut Walk,
    path: &[&str],
    map: &Map<String, Value>,
    known: &[&str],
) {
    for key in map.keys() {
        if !known.contains(&key.as_str()) {
            let mut member = path.to_vec();
            member.push(key);
            walk.fail(&member, code::FIELD_UNKNOWN);
        }
    }
}

/// A string member, possibly empty.
pub(crate) fn text<'a>(
    walk: &mut Walk,
    path: &[&str],
    value: Option<&'a Value>,
) -> Option<&'a str> {
    match value {
        None => {
            walk.fail(path, code::FIELD_REQUIRED);
            None
        }
        Some(Value::String(s)) if s.contains('\0') => {
            walk.fail(path, code::TEXT_CONTAINS_NUL);
            None
        }
        Some(Value::String(s)) => Some(s),
        Some(_) => {
            walk.fail(path, code::FIELD_TYPE);
            None
        }
    }
}

/// A non-empty string member.
pub(crate) fn required_text(
    walk: &mut Walk,
    path: &[&str],
    value: Option<&Value>,
) -> Option<String> {
    let s = text(walk, path, value)?;
    if s.is_empty() {
        walk.fail(path, code::FIELD_EMPTY);
        return None;
    }
    Some(s.to_owned())
}

/// A boolean member, or `default` when it is absent.
pub(crate) fn flag(
    walk: &mut Walk,
    name: &str,
    value: Option<&Value>,
    default: bool,
) -> Option<bool> {
    match value {
        None => Some(default),
        Some(Value::Bool(on)) => Some(*on),
        Some(_) => {
            walk.fail(&[name], code::FIELD_TYPE);
            None
        }
    }
}

/// An absolute `http(s)` URL with no fragment, whitespace or control character, kept exactly as
/// written; a query only when `allow_query`. `policy` is `None` for a stored config, which the
/// client checks again at every connection.
pub(crate) fn url(
    walk: &mut Walk,
    name: &str,
    value: Option<&Value>,
    policy: Option<UrlPolicy>,
    allow_query: bool,
) -> Option<String> {
    let s = text(walk, &[name], value)?;
    // The URL parser drops surrounding blanks and inner tabs and newlines, so the parsed URL
    // would not be the string kept; such a string is refused instead.
    let plain = !s.chars().any(|c| c.is_whitespace() || c.is_control());
    let parsed = Url::parse(s).ok().filter(|url| {
        plain
            && matches!(url.scheme(), "https" | "http")
            && (allow_query || url.query().is_none())
            && url.fragment().is_none()
    });
    let Some(parsed) = parsed else {
        walk.fail(&[name], code::URL_INVALID);
        return None;
    };
    if let Some(policy) = policy
        && policy.check_url(&parsed).is_err()
    {
        walk.fail(&[name], code::URL_REFUSED);
        return None;
    }
    Some(s.to_owned())
}

/// RFC 6749 3.3: `scope-token = 1*( %x21 / %x23-5B / %x5D-7E )`.
fn is_scope_token(scope: &str) -> bool {
    !scope.is_empty()
        && scope
            .bytes()
            .all(|b| matches!(b, 0x21 | 0x23..=0x5B | 0x5D..=0x7E))
}

/// A list of scope tokens, or `default` when `scopes` is absent.
pub(crate) fn scopes(
    walk: &mut Walk,
    value: Option<&Value>,
    default: &[&str],
) -> Option<Vec<String>> {
    let items = match value {
        None => return Some(default.iter().map(|s| (*s).to_owned()).collect()),
        Some(Value::Array(items)) => items,
        Some(_) => {
            walk.fail(&["scopes"], code::FIELD_TYPE);
            return None;
        }
    };
    let before = walk.count();
    let mut scopes = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let index = index.to_string();
        match item {
            Value::String(scope) if is_scope_token(scope) => scopes.push(scope.clone()),
            Value::String(_) => walk.fail(&["scopes", &index], code::SCOPE_INVALID),
            _ => walk.fail(&["scopes", &index], code::FIELD_TYPE),
        }
    }
    (walk.count() == before).then_some(scopes)
}

/// A JSON Pointer member.
pub(crate) fn pointer(walk: &mut Walk, path: &[&str], value: &Value) -> Option<JsonPointer> {
    let s = text(walk, path, Some(value))?;
    match JsonPointer::parse(s) {
        Ok(pointer) => Some(pointer),
        Err(_) => {
            walk.fail(path, code::POINTER_INVALID);
            None
        }
    }
}

/// `claims.<name>` as a pointer, `Some(None)` when it is absent.
pub(crate) fn optional_pointer(
    walk: &mut Walk,
    claims: &Map<String, Value>,
    name: &str,
) -> Option<Option<JsonPointer>> {
    match claims.get(name) {
        None => Some(None),
        Some(value) => pointer(walk, &["claims", name], value).map(Some),
    }
}

/// `claims.attributes`: an object of names to pointers, empty when absent.
pub(crate) fn attributes(
    walk: &mut Walk,
    value: Option<&Value>,
) -> Option<BTreeMap<String, JsonPointer>> {
    let map = match value {
        None => return Some(BTreeMap::new()),
        Some(Value::Object(map)) => map,
        Some(_) => {
            walk.fail(&["claims", "attributes"], code::FIELD_TYPE);
            return None;
        }
    };
    let before = walk.count();
    let mut attributes = BTreeMap::new();
    for (name, value) in map {
        let path = ["claims", "attributes", name.as_str()];
        if name.is_empty() {
            walk.fail(&path, code::FIELD_EMPTY);
        } else if name.contains('\0') {
            walk.fail(&path, code::TEXT_CONTAINS_NUL);
        } else if let Some(pointer) = pointer(walk, &path, value) {
            attributes.insert(name.clone(), pointer);
        }
    }
    (walk.count() == before).then_some(attributes)
}

/// The admin's `client_secret`: a new non-empty string, or the stored secret when the member is
/// absent. A new provider has none stored and must give one.
pub(crate) fn input_secret(
    walk: &mut Walk,
    value: Option<&Value>,
    stored: Option<SealedSecret>,
) -> Option<DraftSecret> {
    match (value, stored) {
        (None, Some(sealed)) => Some(DraftSecret::Kept(sealed)),
        (None, None) => {
            walk.fail(&["client_secret"], code::FIELD_REQUIRED);
            None
        }
        (Some(Value::String(s)), _) if s.is_empty() => {
            walk.fail(&["client_secret"], code::FIELD_EMPTY);
            None
        }
        (Some(Value::String(s)), _) => Some(DraftSecret::New(ClientSecret(s.clone()))),
        (Some(_), _) => {
            walk.fail(&["client_secret"], code::FIELD_TYPE);
            None
        }
    }
}

/// A stored config's secret: `client_secret: {"sealed": "<standard base64>"}`, or, for a row
/// with `source: "config"`, no `client_secret` at all. Any other `source` fails closed.
pub(crate) fn stored_secret(walk: &mut Walk, config: &Map<String, Value>) -> Option<StoredSecret> {
    let value = config.get("client_secret");
    match config.get(SOURCE) {
        None => {}
        Some(Value::String(source)) if source == SOURCE_CONFIG => {
            if value.is_some() {
                walk.fail(&["client_secret"], code::FIELD_UNKNOWN);
                return None;
            }
            return Some(StoredSecret::ConfigFile);
        }
        Some(_) => {
            walk.fail(&[SOURCE], code::FIELD_TYPE);
            return None;
        }
    }
    let sealed = value
        .and_then(Value::as_object)
        .and_then(|secret| secret.get("sealed"))
        .and_then(Value::as_str)
        .and_then(|text| STANDARD.decode(text).ok());
    if sealed.is_none() {
        walk.fail(&["client_secret"], code::FIELD_TYPE);
    }
    sealed.map(|bytes| StoredSecret::Sealed(SealedSecret(bytes)))
}

/// Put `secret`'s stored form in `config`, which [`stored_secret`] reads.
pub(crate) fn insert_stored_secret(config: &mut Map<String, Value>, secret: &StoredSecret) {
    match secret {
        StoredSecret::Sealed(sealed) => {
            config.insert("client_secret".into(), sealed_json(sealed));
        }
        StoredSecret::ConfigFile => {
            config.insert(SOURCE.into(), SOURCE_CONFIG.into());
        }
    }
}

/// The stored form of a sealed secret, which [`stored_secret`] reads.
fn sealed_json(secret: &SealedSecret) -> Value {
    let mut sealed = Map::new();
    sealed.insert("sealed".into(), STANDARD.encode(&secret.0).into());
    Value::Object(sealed)
}

/// The stored form of `claims.attributes`.
pub(crate) fn attributes_json(attributes: &BTreeMap<String, JsonPointer>) -> Value {
    Value::Object(
        attributes
            .iter()
            .map(|(name, pointer)| (name.clone(), pointer.as_str().into()))
            .collect(),
    )
}
