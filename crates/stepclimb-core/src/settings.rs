// SPDX-License-Identifier: AGPL-3.0-or-later
//! The typed form of `organizations.settings` (spec 02 B14).
//!
//! The column is read and written only through these types. Each key is owned by the spec that
//! uses it and is added here with its default in code (`#[serde(default)]` on the struct), so a
//! stored object missing a key reads as that default and no migration backfills settings. M0's
//! keys arrive with specs 03, 04 and 07; until then both structs are empty.
//!
//! A stored key this code does not know is an error on read, not a value to drop: the column
//! is only ever written from an [`OrgSettingsPatch`], so such a key means the data and the code
//! disagree. Removing a key therefore takes a migration that strips it from every row.

use serde::{Deserialize, Serialize};

/// Every organization setting with its effective value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
    fn a_stored_key_the_code_does_not_know_is_an_error() {
        let read = serde_json::from_str::<OrgSettings>(r#"{"no_such_key": true}"#);
        assert!(
            read.is_err(),
            "an unknown stored key is refused, not dropped"
        );
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
