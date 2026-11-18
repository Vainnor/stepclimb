// SPDX-License-Identifier: AGPL-3.0-or-later
//! The typed form of `organizations.settings` (spec 02 B14).
//!
//! The column is read and written only through these types. Each key is owned by the spec that
//! uses it and is added here with its default in code (`#[serde(default)]` on the struct), so a
//! stored object missing a key reads as that default and no migration backfills settings. Spec
//! 03 owns `default_tz` and `email_matching`, spec 04 `local_sign_in`, spec 07 `admission`, and
//! M1 spec 03 `embedding`.
//!
//! Stored settings are read tolerantly: a key this code does not know is ignored on read and
//! kept in the row, because the repository merges a patch into the stored object
//! (`settings || patch`) instead of replacing it. Migrations are forward-only (expand, then
//! contract), so during a rolling deploy or after a rollback a server can meet a key a newer
//! release wrote; it must neither fail nor drop it. A patch is strict: it refuses unknown keys
//! (spec 02 B14, `settings.unknown_key`).

use serde::{Deserialize, Serialize};

use crate::local_sign_in::LocalMethods;

/// Every organization setting with its effective value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OrgSettings {
    /// The IANA time zone of a user created without one ([`crate::tz`]; spec 03 B3, Q4).
    pub default_tz: String,
    /// Whether a sign-in through an identity the organization does not know may be linked to
    /// an existing user with the same email address (spec 03 B17, Q2 (a); spec 07 B5). Off by
    /// default.
    pub email_matching: bool,
    /// The local sign-in method switches (spec 04 B1, Q2), written only through
    /// [`crate::local_sign_in::update`].
    pub local_sign_in: LocalMethods,
    /// The admission switches (spec 07 B12, B20, Q2), written only through
    /// [`crate::admission_settings::update`].
    pub admission: AdmissionSwitches,
    /// How host sites link into the organization's courses (M1 spec 03 B17).
    pub embedding: Embedding,
}

impl Default for OrgSettings {
    fn default() -> Self {
        Self {
            default_tz: crate::tz::DEFAULT.to_owned(),
            email_matching: false,
            local_sign_in: LocalMethods::default(),
            admission: AdmissionSwitches::default(),
            embedding: Embedding::default(),
        }
    }
}

/// The most origins [`Embedding::return_origins`] holds (M1 spec 03 B17).
pub const MAX_RETURN_ORIGINS: usize = 20;

/// How host sites link into the organization's courses (M1 spec 03 B16, B17).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Embedding {
    /// The origins a deep link's `return` URL may point at for the web app to show its "Back
    /// to" link, each in the form [`return_origin`] gives: `https://host[:port]`, lowercase,
    /// without a path. Empty by default, so no return link shows.
    pub return_origins: Vec<String>,
}

/// The whole new [`Embedding`]: every member is required, so a patch naming `embedding`
/// replaces each one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingPatch {
    /// At most [`MAX_RETURN_ORIGINS`] entries, each accepted by [`return_origin`]; the
    /// repository stores their canonical forms, each once, in the order given.
    pub return_origins: Vec<String>,
}

/// `entry`'s stored form when it is an `https` origin written as one, `https://host[:port]`,
/// otherwise `None` (M1 spec 03 B17). The entry must equal, in any ASCII letter case, the
/// origin the URL parser reads from it, so a path (even a lone `/`), a query, a fragment, a user
/// name or password, a default port written out (`:443`), white space and any non-ASCII host
/// (written other than as its `xn--` form) are refused rather than cut away. The stored form is
/// that origin, lowercase.
pub fn return_origin(entry: &str) -> Option<String> {
    let parsed = url::Url::parse(entry).ok()?;
    // An `https` URL always has a host, so its origin is a tuple origin, never opaque.
    if parsed.scheme() != "https" {
        return None;
    }
    let origin = parsed.origin().ascii_serialization();
    entry.eq_ignore_ascii_case(&origin).then_some(origin)
}

/// Who admission lets in (spec 07). Each switch is off unless stored on, so a stored object
/// missing one reads as off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AdmissionSwitches {
    /// Whether a person no user of the organization matches is created as a user at sign-in
    /// (spec 07 B20); off, they are refused with `auth.signup_closed` (B21).
    pub signup_allowed: bool,
    /// Whether only users already provisioned may sign in (spec 07 B11, B12); a person no user
    /// matches is then refused with `auth.not_provisioned`, whatever `signup_allowed` says.
    pub sync_only: bool,
}

/// A partial update of [`OrgSettings`]: one optional member per key but those in
/// [`OrgSettingsPatch::WRITTEN_ELSEWHERE`], `None` for "leave as it is". Its JSON form names only
/// the keys being changed, which the repository merges into the stored object in one statement.
/// Unknown keys are refused when it is parsed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrgSettingsPatch {
    /// An IANA time zone name; the repository stores the database's spelling of it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_tz: Option<String>,
    /// Who may set it to `true` is the settings route's check (spec 07 I4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_matching: Option<bool>,
    /// The repository checks each origin with [`return_origin`] and stores its canonical form.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding: Option<EmbeddingPatch>,
}

impl OrgSettingsPatch {
    /// The settings keys no patch can name, because one repository function writes each with
    /// rules of its own: `local_sign_in` through [`crate::local_sign_in::update`] (spec 04 Q2),
    /// and `admission` through [`crate::admission_settings::update`] (spec 07 Q2).
    pub const WRITTEN_ELSEWHERE: &'static [&'static str] = &["local_sign_in", "admission"];

    /// Whether a patch may name `key`, so the API can report each unknown key by name
    /// (`settings.unknown_key`) instead of failing the whole parse. It asks the type itself:
    /// every member is optional, so `{ key: null }` parses exactly when `key` is a member, and
    /// a key added to the struct is known here without a second list to keep in step.
    pub fn knows(key: &str) -> bool {
        let probe = serde_json::Map::from_iter([(key.to_owned(), serde_json::Value::Null)]);
        serde_json::from_value::<Self>(serde_json::Value::Object(probe)).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_stored_object_reads_as_the_defaults() {
        let settings: OrgSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, OrgSettings::default());
    }

    #[test]
    fn a_stored_key_the_code_does_not_know_is_ignored_on_read() {
        let settings: OrgSettings = serde_json::from_str(r#"{"newer_key": true}"#).unwrap();
        assert_eq!(settings, OrgSettings::default());
    }

    #[test]
    fn a_patch_with_an_unknown_key_is_refused() {
        let parsed = serde_json::from_str::<OrgSettingsPatch>(r#"{"no_such_key": true}"#);
        assert!(parsed.is_err());
    }

    #[test]
    fn a_patch_knows_every_settings_key_but_those_written_elsewhere() {
        let serde_json::Value::Object(defaults) =
            serde_json::to_value(OrgSettings::default()).unwrap()
        else {
            panic!("the settings serialize to an object");
        };
        for key in defaults.keys() {
            let elsewhere = OrgSettingsPatch::WRITTEN_ELSEWHERE.contains(&key.as_str());
            assert_eq!(OrgSettingsPatch::knows(key), !elsewhere, "{key}");
        }
        for key in OrgSettingsPatch::WRITTEN_ELSEWHERE {
            assert!(defaults.contains_key(*key), "{key} is a setting");
        }
        assert!(!OrgSettingsPatch::knows("no_such_key"));
        assert!(!OrgSettingsPatch::knows(""));
    }

    #[test]
    fn the_default_time_zone_is_utc() {
        assert_eq!(OrgSettings::default().default_tz, "UTC");
        let stored: OrgSettings =
            serde_json::from_str(r#"{"default_tz": "Europe/Dublin"}"#).unwrap();
        assert_eq!(stored.default_tz, "Europe/Dublin");
    }

    #[test]
    fn email_matching_is_off_by_default_and_reads_a_stored_value() {
        assert!(!OrgSettings::default().email_matching);
        let settings: OrgSettings = serde_json::from_str(r#"{"default_tz": "UTC"}"#).unwrap();
        assert!(!settings.email_matching);
        let stored: OrgSettings = serde_json::from_str(r#"{"email_matching": true}"#).unwrap();
        assert!(stored.email_matching);
    }

    #[test]
    fn sign_up_is_closed_by_default_and_reads_a_stored_value() {
        assert!(!OrgSettings::default().admission.signup_allowed);
        let settings: OrgSettings = serde_json::from_str(r#"{"admission": {}}"#).unwrap();
        assert!(!settings.admission.signup_allowed);
        let stored: OrgSettings =
            serde_json::from_str(r#"{"admission": {"signup_allowed": true, "newer": 1}}"#).unwrap();
        assert!(stored.admission.signup_allowed);
    }

    #[test]
    fn sync_only_is_off_by_default_and_reads_a_stored_value() {
        assert!(!OrgSettings::default().admission.sync_only);
        let settings: OrgSettings =
            serde_json::from_str(r#"{"admission": {"signup_allowed": true}}"#).unwrap();
        assert!(!settings.admission.sync_only);
        let stored: OrgSettings =
            serde_json::from_str(r#"{"admission": {"sync_only": true}}"#).unwrap();
        assert!(stored.admission.sync_only);
        assert!(!stored.admission.signup_allowed);
    }

    #[test]
    fn a_patch_naming_the_admission_switches_is_refused() {
        let parsed =
            serde_json::from_str::<OrgSettingsPatch>(r#"{"admission": {"signup_allowed": true}}"#);
        assert!(parsed.is_err());
        assert!(!OrgSettingsPatch::knows("admission"));
    }

    #[test]
    fn a_patch_serializes_only_the_keys_it_names() {
        let patch = OrgSettingsPatch {
            default_tz: Some("Europe/Dublin".into()),
            ..OrgSettingsPatch::default()
        };
        let json = serde_json::to_string(&patch).unwrap();
        assert_eq!(json, r#"{"default_tz":"Europe/Dublin"}"#);
        let patch = OrgSettingsPatch {
            email_matching: Some(false),
            ..OrgSettingsPatch::default()
        };
        let json = serde_json::to_string(&patch).unwrap();
        assert_eq!(json, r#"{"email_matching":false}"#);
    }

    #[test]
    fn return_origins_are_empty_by_default_and_read_a_stored_list() {
        assert!(OrgSettings::default().embedding.return_origins.is_empty());
        let stored: OrgSettings = serde_json::from_str(
            r#"{"embedding": {"return_origins": ["https://host.example"], "newer": 1}}"#,
        )
        .unwrap();
        assert_eq!(stored.embedding.return_origins, ["https://host.example"]);
    }

    #[test]
    fn an_embedding_patch_names_every_member_and_nothing_else() {
        let parse = |json| serde_json::from_str::<OrgSettingsPatch>(json);
        assert!(
            parse(r#"{"embedding": {}}"#).is_err(),
            "return_origins is required"
        );
        assert!(parse(r#"{"embedding": {"return_origins": [], "x": 1}}"#).is_err());
        let patch = parse(r#"{"embedding": {"return_origins": ["https://a.example"]}}"#).unwrap();
        let origins = patch.embedding.map(|e| e.return_origins);
        assert_eq!(origins, Some(vec!["https://a.example".to_owned()]));
        assert!(OrgSettingsPatch::knows("embedding"));
    }

    /// M1 spec 03 T11 and the forms around it: only `https://host[:port]` as written.
    #[test]
    fn a_return_origin_is_an_https_origin_written_as_one() {
        let accepted = [
            ("https://host.example", "https://host.example"),
            ("https://Host.EXAMPLE", "https://host.example"),
            ("HTTPS://host.example:8443", "https://host.example:8443"),
            ("https://127.0.0.1", "https://127.0.0.1"),
            ("https://[::1]:8443", "https://[::1]:8443"),
            (
                "https://xn--bcher-kva.example",
                "https://xn--bcher-kva.example",
            ),
        ];
        for (entry, stored) in accepted {
            assert_eq!(return_origin(entry).as_deref(), Some(stored), "{entry}");
        }
        let refused = [
            "http://host.example",
            "https://host.example/path",
            "https://host.example/",
            "https://host.example:443",
            "https://host.example?x=1",
            "https://host.example#top",
            "https://user@host.example",
            "https://user:pw@host.example",
            " https://host.example",
            "https://host.example\t",
            "https://host\u{0009}.example",
            "https://b\u{fc}cher.example",
            "https://host.example\u{200b}",
            "//host.example",
            "host.example",
            "https:host.example",
            "https://",
            "javascript:alert(1)",
            "",
        ];
        for entry in refused {
            assert_eq!(return_origin(entry), None, "{entry:?}");
        }
    }

    #[test]
    fn an_empty_patch_writes_an_empty_object() {
        let json = serde_json::to_string(&OrgSettingsPatch::default()).unwrap();
        assert_eq!(json, "{}");
    }
}
