// SPDX-License-Identifier: AGPL-3.0-or-later
//! The typed form of `organizations.settings` (spec 02 B14).
//!
//! The column is read and written only through these types. Each key is owned by the spec that
//! uses it and is added here with its default in code (`#[serde(default)]` on the struct), so a
//! stored object missing a key reads as that default and no migration backfills settings. Spec
//! 03 owns `default_tz`; specs 04 and 07 add theirs.
//!
//! Stored settings are read tolerantly: a key this code does not know is ignored on read and
//! kept in the row, because the repository merges a patch into the stored object
//! (`settings || patch`) instead of replacing it. Migrations are forward-only (expand, then
//! contract), so during a rolling deploy or after a rollback a server can meet a key a newer
//! release wrote; it must neither fail nor drop it. A patch is strict: it refuses unknown keys
//! (spec 02 B14, `settings.unknown_key`).

use serde::{Deserialize, Serialize};

/// Every organization setting with its effective value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OrgSettings {
    /// The IANA time zone of a user created without one ([`crate::tz`]; spec 03 B3, Q4).
    pub default_tz: String,
}

impl Default for OrgSettings {
    fn default() -> Self {
        Self {
            default_tz: crate::tz::DEFAULT.to_owned(),
        }
    }
}

/// A partial update of [`OrgSettings`]: one optional member per key, `None` for "leave as it
/// is". Its JSON form names only the keys being changed, which the repository merges into the
/// stored object in one statement. Unknown keys are refused when it is parsed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrgSettingsPatch {
    /// An IANA time zone name; the repository stores the database's spelling of it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_tz: Option<String>,
}

impl OrgSettingsPatch {
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
    fn a_patch_knows_every_settings_key_and_no_other() {
        let serde_json::Value::Object(defaults) =
            serde_json::to_value(OrgSettings::default()).unwrap()
        else {
            panic!("the settings serialize to an object");
        };
        for key in defaults.keys() {
            assert!(OrgSettingsPatch::knows(key), "{key}");
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
    fn a_patch_serializes_only_the_keys_it_names() {
        let patch = OrgSettingsPatch {
            default_tz: Some("Europe/Dublin".into()),
        };
        let json = serde_json::to_string(&patch).unwrap();
        assert_eq!(json, r#"{"default_tz":"Europe/Dublin"}"#);
    }

    #[test]
    fn an_empty_patch_writes_an_empty_object() {
        let json = serde_json::to_string(&OrgSettingsPatch::default()).unwrap();
        assert_eq!(json, "{}");
    }
}
