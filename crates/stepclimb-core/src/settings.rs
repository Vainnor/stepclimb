// SPDX-License-Identifier: AGPL-3.0-or-later
//! The typed form of `organizations.settings` (spec 02 B14).
//!
//! The column is read and written only through these types. Each key is owned by the spec that
//! uses it and is added here with its default in code (`#[serde(default)]` on the struct), so a
//! stored object missing a key reads as that default and no migration backfills settings. M0's
//! keys arrive with specs 03, 04 and 07; until then both structs are empty.
//!
//! Stored settings are read tolerantly: a key this code does not know is ignored on read and
//! kept in the row, because the repository merges a patch into the stored object
//! (`settings || patch`) instead of replacing it. Migrations are forward-only (expand, then
//! contract), so during a rolling deploy or after a rollback a server can meet a key a newer
//! release wrote; it must neither fail nor drop it. A patch is strict: it refuses unknown keys
//! (spec 02 B14, `settings.unknown_key`).

use serde::{Deserialize, Serialize};

/// Every organization setting with its effective value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OrgSettings {}

/// A partial update of [`OrgSettings`]: one optional member per key, `None` for "leave as it
/// is". Its JSON form names only the keys being changed, which the repository merges into the
/// stored object in one statement. Unknown keys are refused when it is parsed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrgSettingsPatch {}

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
    fn an_empty_patch_writes_an_empty_object() {
        let json = serde_json::to_string(&OrgSettingsPatch::default()).unwrap();
        assert_eq!(json, "{}");
    }
}
