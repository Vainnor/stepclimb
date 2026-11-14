// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rules of the module docs' table, member by member, collecting every problem.

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value};
use url::Url;

use super::{
    ClaimsMapping, ClientSecret, ConfigErrors, ConfigProblem, DEFAULT_SCOPES, DraftSecret,
    OidcConfig, OidcConfigDraft, OidcSettings, SealedSecret, StoredConfigError, code,
};
use crate::json_pointer::{JsonPointer, pointer_to};
use crate::outbound::UrlPolicy;

/// The members of an `oidc` config.
const MEMBERS: [&str; 6] = [
    "issuer",
    "client_id",
    "client_secret",
    "scopes",
    "claims",
    "userinfo",
];

/// The members of `claims`.
const CLAIMS: [&str; 6] = [
    "given_name",
    "family_name",
    "email",
    "email_verified",
    "groups",
    "attributes",
];

#[derive(Default)]
struct Walk {
    problems: Vec<ConfigProblem>,
}

impl Walk {
    fn fail(&mut self, path: &[&str], code: &'static str) {
        self.problems.push(ConfigProblem {
            pointer: pointer_to(path),
            code,
        });
    }
}

pub(super) fn draft(
    config: &Value,
    policy: UrlPolicy,
    stored: Option<SealedSecret>,
) -> Result<OidcConfigDraft, ConfigErrors> {
    let mut walk = Walk::default();
    let Some(map) = config.as_object() else {
        walk.fail(&[], code::FIELD_TYPE);
        return Err(ConfigErrors(walk.problems));
    };
    for key in map.keys() {
        if !MEMBERS.contains(&key.as_str()) {
            walk.fail(&[key], code::FIELD_UNKNOWN);
        }
    }
    let settings = settings(&mut walk, map, Some(policy));
    let secret = input_secret(&mut walk, map.get("client_secret"), stored);
    match (settings, secret) {
        (Some(settings), Some(secret)) if walk.problems.is_empty() => {
            Ok(OidcConfigDraft { settings, secret })
        }
        _ => Err(ConfigErrors(walk.problems)),
    }
}

pub(super) fn stored(config: &Map<String, Value>) -> Result<OidcConfig, StoredConfigError> {
    let mut walk = Walk::default();
    let settings = settings(&mut walk, config, None);
    let secret = stored_secret(&mut walk, config.get("client_secret"));
    match (settings, secret) {
        (Some(settings), Some(secret)) if walk.problems.is_empty() => {
            Ok(OidcConfig { settings, secret })
        }
        _ => {
            let first = walk.problems.into_iter().next();
            let (pointer, code) = first.map_or_else(
                || (String::new(), code::FIELD_TYPE),
                |p| (p.pointer, p.code),
            );
            Err(StoredConfigError::Invalid { pointer, code })
        }
    }
}

/// Every member but the secret. `policy` is `None` for a stored config.
fn settings(
    walk: &mut Walk,
    map: &Map<String, Value>,
    policy: Option<UrlPolicy>,
) -> Option<OidcSettings> {
    let issuer = issuer(walk, map.get("issuer"), policy);
    let client_id = required_text(walk, "client_id", map.get("client_id"));
    let scopes = scopes(walk, map.get("scopes"));
    let claims = claims(walk, map.get("claims"));
    let userinfo = match map.get("userinfo") {
        None => Some(true),
        Some(Value::Bool(on)) => Some(*on),
        Some(_) => {
            walk.fail(&["userinfo"], code::FIELD_TYPE);
            None
        }
    };
    Some(OidcSettings {
        issuer: issuer?,
        client_id: client_id?,
        scopes: scopes?,
        claims: claims?,
        userinfo: userinfo?,
    })
}

fn text<'a>(walk: &mut Walk, path: &[&str], value: Option<&'a Value>) -> Option<&'a str> {
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

fn required_text(walk: &mut Walk, name: &str, value: Option<&Value>) -> Option<String> {
    let s = text(walk, &[name], value)?;
    if s.is_empty() {
        walk.fail(&[name], code::FIELD_EMPTY);
        return None;
    }
    Some(s.to_owned())
}

/// The issuer exactly as written, once it parses as an issuer URL and passes `policy`.
fn issuer(walk: &mut Walk, value: Option<&Value>, policy: Option<UrlPolicy>) -> Option<String> {
    let s = text(walk, &["issuer"], value)?;
    // The URL parser drops surrounding blanks and inner tabs and newlines, so the parsed URL
    // would not be the string kept; such a string is refused instead.
    let plain = !s.chars().any(|c| c.is_whitespace() || c.is_control());
    let url = Url::parse(s).ok().filter(|url| {
        plain
            && matches!(url.scheme(), "https" | "http")
            && url.query().is_none()
            && url.fragment().is_none()
    });
    let Some(url) = url else {
        walk.fail(&["issuer"], code::URL_INVALID);
        return None;
    };
    if let Some(policy) = policy
        && policy.check_url(&url).is_err()
    {
        walk.fail(&["issuer"], code::URL_REFUSED);
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

fn scopes(walk: &mut Walk, value: Option<&Value>) -> Option<Vec<String>> {
    let items = match value {
        None => return Some(DEFAULT_SCOPES.map(str::to_owned).to_vec()),
        Some(Value::Array(items)) => items,
        Some(_) => {
            walk.fail(&["scopes"], code::FIELD_TYPE);
            return None;
        }
    };
    let before = walk.problems.len();
    let mut scopes = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let index = index.to_string();
        match item {
            Value::String(scope) if is_scope_token(scope) => scopes.push(scope.clone()),
            Value::String(_) => walk.fail(&["scopes", &index], code::SCOPE_INVALID),
            _ => walk.fail(&["scopes", &index], code::FIELD_TYPE),
        }
    }
    if walk.problems.len() > before {
        return None;
    }
    if !scopes.iter().any(|scope| scope == "openid") {
        walk.fail(&["scopes"], code::SCOPES_MISSING_OPENID);
        return None;
    }
    Some(scopes)
}

fn pointer(walk: &mut Walk, path: &[&str], value: &Value) -> Option<JsonPointer> {
    let s = text(walk, path, Some(value))?;
    match JsonPointer::parse(s) {
        Ok(pointer) => Some(pointer),
        Err(_) => {
            walk.fail(path, code::POINTER_INVALID);
            None
        }
    }
}

/// A profile field's pointer, or B13's default `/<name>` when the member is absent.
fn profile_pointer(walk: &mut Walk, map: &Map<String, Value>, name: &str) -> Option<JsonPointer> {
    match map.get(name) {
        Some(value) => pointer(walk, &["claims", name], value),
        None => JsonPointer::parse(&format!("/{name}")).ok(),
    }
}

fn claims(walk: &mut Walk, value: Option<&Value>) -> Option<ClaimsMapping> {
    let empty = Map::new();
    let map = match value {
        None => &empty,
        Some(Value::Object(map)) => map,
        Some(_) => {
            walk.fail(&["claims"], code::FIELD_TYPE);
            return None;
        }
    };
    for key in map.keys() {
        if !CLAIMS.contains(&key.as_str()) {
            walk.fail(&["claims", key], code::FIELD_UNKNOWN);
        }
    }
    let given_name = profile_pointer(walk, map, "given_name");
    let family_name = profile_pointer(walk, map, "family_name");
    let email = profile_pointer(walk, map, "email");
    let email_verified = profile_pointer(walk, map, "email_verified");
    let groups = match map.get("groups") {
        None => Some(None),
        Some(value) => pointer(walk, &["claims", "groups"], value).map(Some),
    };
    let attributes = attributes(walk, map.get("attributes"));
    Some(ClaimsMapping {
        given_name: given_name?,
        family_name: family_name?,
        email: email?,
        email_verified: email_verified?,
        groups: groups?,
        attributes: attributes?,
    })
}

fn attributes(walk: &mut Walk, value: Option<&Value>) -> Option<BTreeMap<String, JsonPointer>> {
    let map = match value {
        None => return Some(BTreeMap::new()),
        Some(Value::Object(map)) => map,
        Some(_) => {
            walk.fail(&["claims", "attributes"], code::FIELD_TYPE);
            return None;
        }
    };
    let before = walk.problems.len();
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
    (walk.problems.len() == before).then_some(attributes)
}

fn input_secret(
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

/// `{"sealed": "<standard base64>"}`.
fn stored_secret(walk: &mut Walk, value: Option<&Value>) -> Option<SealedSecret> {
    let sealed = value
        .and_then(Value::as_object)
        .and_then(|secret| secret.get("sealed"))
        .and_then(Value::as_str)
        .and_then(|text| STANDARD.decode(text).ok());
    if sealed.is_none() {
        walk.fail(&["client_secret"], code::FIELD_TYPE);
    }
    sealed.map(SealedSecret)
}
