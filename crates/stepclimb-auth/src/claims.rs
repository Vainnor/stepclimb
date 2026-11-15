// SPDX-License-Identifier: AGPL-3.0-or-later
//! Claim mapping: reading a sign-in's subject, profile fields, groups and attributes out of a
//! provider's JSON by JSON Pointer (`docs/specs/m0/06-oauth2-and-vatsim-preset.md` B6,
//! `05-oidc.md` B13). One module for both provider kinds: an `oauth2` provider maps its userinfo
//! body ([`crate::oauth2_config::Oauth2Claims::map`]), an `oidc` provider its merged ID token
//! and userinfo claims ([`crate::oidc_config::ClaimsMapping::profile`]).
//!
//! | Field | Kept when the pointer names | Otherwise |
//! | --- | --- | --- |
//! | subject (`oauth2` only) | a non-empty string, or an integer as its decimal string | the sign-in fails with [`SubjectMissing`] |
//! | `given_name`, `family_name`, `email` | a non-empty string | absent |
//! | `email_verified` | a boolean | absent |
//! | `groups` | an array of strings | absent |
//! | `attributes.<name>` | any JSON value | the attribute is absent |
//!
//! An unmapped field and a pointer that names nothing are absent too. Values are kept as the
//! provider sent them, without trimming or case folding.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::{Map, Value};

use crate::json_pointer::JsonPointer;

/// The redirect error code of a sign-in whose subject is missing or unusable (B6).
pub const SUBJECT_MISSING: &str = "auth.subject_missing";

/// The subject pointer named nothing, or a value that is neither a non-empty string nor an
/// integer. The error carries no value: it may be logged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the provider's response has no usable subject ({SUBJECT_MISSING})")]
pub struct SubjectMissing;

/// The subject `pointer` names in `document`: a non-empty string as it is, or an integer as its
/// decimal string, so `1000001` and `"1000001"` are the same subject. A float, even a whole one
/// such as `1000001.0`, is not an integer.
pub fn subject(document: &Value, pointer: &JsonPointer) -> Result<String, SubjectMissing> {
    match pointer.resolve(document) {
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        Some(Value::Number(n)) if n.is_i64() || n.is_u64() => Ok(n.to_string()),
        _ => Err(SubjectMissing),
    }
}

/// Where the profile fields, groups and attributes come from. `None` leaves a field unmapped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileMapping {
    pub given_name: Option<JsonPointer>,
    pub family_name: Option<JsonPointer>,
    pub email: Option<JsonPointer>,
    pub email_verified: Option<JsonPointer>,
    pub groups: Option<JsonPointer>,
    pub attributes: BTreeMap<String, JsonPointer>,
}

/// What a [`ProfileMapping`] found: the input admission takes besides the subject
/// (`07-admission.md` B1).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct MappedProfile {
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub groups: Option<Vec<String>>,
    pub attributes: Map<String, Value>,
}

/// Shows which fields were found, never their values: they are personal data the provider
/// sent, which logs never carry (`07-admission.md` B2).
impl fmt::Debug for MappedProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MappedProfile")
            .field("given_name", &self.given_name.is_some())
            .field("family_name", &self.family_name.is_some())
            .field("email", &self.email.is_some())
            .field("email_verified", &self.email_verified.is_some())
            .field("groups", &self.groups.as_ref().map(Vec::len))
            .field("attributes", &self.attributes.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Read `mapping` out of `document`, by the rules of the module table.
#[must_use]
pub fn profile(document: &Value, mapping: &ProfileMapping) -> MappedProfile {
    let find = |pointer: &Option<JsonPointer>| pointer.as_ref().and_then(|p| p.resolve(document));
    let text = |pointer: &Option<JsonPointer>| match find(pointer) {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    MappedProfile {
        given_name: text(&mapping.given_name),
        family_name: text(&mapping.family_name),
        email: text(&mapping.email),
        email_verified: find(&mapping.email_verified).and_then(Value::as_bool),
        groups: find(&mapping.groups).and_then(string_list),
        attributes: mapping
            .attributes
            .iter()
            .filter_map(|(name, pointer)| Some((name.clone(), pointer.resolve(document)?.clone())))
            .collect(),
    }
}

/// An array whose every element is a string; anything else is not a group list.
fn string_list(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn ptr(text: &str) -> JsonPointer {
        JsonPointer::parse(text).expect("a pointer")
    }

    /// Spec 06 T4: the string and the number give the same subject.
    #[test]
    fn spec_t4_a_string_or_integer_subject_is_the_same_subject() {
        let cid = ptr("/data/cid");
        let as_string = subject(&json!({ "data": { "cid": "1000001" } }), &cid);
        let as_number = subject(&json!({ "data": { "cid": 1000001 } }), &cid);
        assert_eq!(as_string, Ok("1000001".to_owned()));
        assert_eq!(as_number, as_string);
        assert_eq!(
            subject(&json!({ "data": { "cid": -7 } }), &cid),
            Ok("-7".to_owned())
        );
        assert_eq!(
            subject(&json!({ "data": { "cid": u64::MAX } }), &cid),
            Ok(u64::MAX.to_string())
        );
    }

    /// Spec 06 T4: a boolean, an empty string or a missing subject is `auth.subject_missing`.
    #[test]
    fn spec_t4_any_other_subject_is_missing() {
        let cid = ptr("/data/cid");
        for body in [
            json!({ "data": { "cid": true } }),
            json!({ "data": { "cid": "" } }),
            json!({ "data": {} }),
            json!({}),
            json!({ "data": { "cid": null } }),
            json!({ "data": { "cid": 1000001.0 } }),
            json!({ "data": { "cid": 1.5 } }),
            json!({ "data": { "cid": ["1000001"] } }),
            json!({ "data": { "cid": { "id": 1 } } }),
            json!(["1000001"]),
        ] {
            assert_eq!(subject(&body, &cid), Err(SubjectMissing), "{body}");
        }
        assert_eq!(SUBJECT_MISSING, "auth.subject_missing");
    }

    /// Spec 06 T4: the subject pointer's `~0` and `~1` escapes resolve per RFC 6901.
    #[test]
    fn spec_t4_an_escaped_subject_pointer_resolves() {
        let body = json!({ "a/b": { "m~n": "1000002" } });
        assert_eq!(subject(&body, &ptr("/a~1b/m~0n")), Ok("1000002".to_owned()));
    }

    fn full_mapping() -> ProfileMapping {
        ProfileMapping {
            given_name: Some(ptr("/p/first")),
            family_name: Some(ptr("/p/last")),
            email: Some(ptr("/p/email")),
            email_verified: Some(ptr("/p/verified")),
            groups: Some(ptr("/groups")),
            attributes: BTreeMap::from([
                ("rating".to_owned(), ptr("/r/short")),
                ("roles".to_owned(), ptr("/roles")),
                ("absent".to_owned(), ptr("/nothing/here")),
            ]),
        }
    }

    #[test]
    fn every_field_is_read_by_its_pointer() {
        let body = json!({
            "p": { "first": "Test", "last": "Student", "email": "student@example.test", "verified": true },
            "groups": ["alpha", "beta"],
            "r": { "short": "S1" },
            "roles": [{ "id": 3 }],
        });
        let mapped = profile(&body, &full_mapping());
        assert_eq!(mapped.given_name.as_deref(), Some("Test"));
        assert_eq!(mapped.family_name.as_deref(), Some("Student"));
        assert_eq!(mapped.email.as_deref(), Some("student@example.test"));
        assert_eq!(mapped.email_verified, Some(true));
        assert_eq!(mapped.groups, Some(vec!["alpha".into(), "beta".into()]));
        // An attribute keeps the JSON value it points to; one that names nothing is absent.
        let expected = json!({ "rating": "S1", "roles": [{ "id": 3 }] });
        assert_eq!(Value::Object(mapped.attributes), expected);
    }

    #[test]
    fn a_value_of_the_wrong_type_is_absent() {
        let body = json!({
            "p": { "first": 42, "last": "", "email": ["a@example.test"], "verified": "true" },
            "groups": ["alpha", 2],
        });
        let mapped = profile(&body, &full_mapping());
        assert_eq!(mapped.given_name, None);
        assert_eq!(mapped.family_name, None);
        assert_eq!(mapped.email, None);
        assert_eq!(mapped.email_verified, None);
        assert_eq!(mapped.groups, None);
        let single = profile(&json!({ "groups": "alpha" }), &full_mapping());
        assert_eq!(single.groups, None);
    }

    #[test]
    fn an_unmapped_field_is_absent_whatever_the_document_holds() {
        let body = json!({ "given_name": "Test", "email": "student@example.test", "email_verified": true });
        assert_eq!(
            profile(&body, &ProfileMapping::default()),
            MappedProfile::default()
        );
    }

    #[test]
    fn debug_shows_no_value() {
        let body = json!({
            "p": { "first": "Test", "email": "student@example.test" },
            "r": { "short": "S1" },
        });
        let shown = format!("{:?}", profile(&body, &full_mapping()));
        for value in ["Test", "student@example.test", "S1"] {
            assert!(!shown.contains(value), "{shown}");
        }
        assert!(shown.contains("rating"), "{shown}");
    }
}
