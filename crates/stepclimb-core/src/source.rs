// SPDX-License-Identifier: AGPL-3.0-or-later
//! Where a record came from (docs/specs/m0/03-users-and-identities.md B4).
//!
//! One enum for every record an outside system can own: users, role assignments (spec 09),
//! enrollments (M1 spec 02) and, in M4, groups. A record whose source is not [`Source::Local`] belongs to that source, and the
//! admin API refuses changes to its source-owned fields (spec 03 B6). The stored spellings are
//! [`Source::as_str`]; each table's `source` column has a check constraint listing the same five.

/// Where a record came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// Made in Stepclimb, by an admin.
    Local,
    /// Made just-in-time at sign-in through an identity provider.
    Idp,
    /// Written by the sync API (M4).
    Sync,
    /// Written by SCIM (M4).
    Scim,
    /// Written by a connector (M4).
    Connector,
}

impl Source {
    /// Every source, in the order the check constraints list them.
    pub const ALL: [Self; 5] = [
        Self::Local,
        Self::Idp,
        Self::Sync,
        Self::Scim,
        Self::Connector,
    ];

    /// The stored spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Idp => "idp",
            Self::Sync => "sync",
            Self::Scim => "scim",
            Self::Connector => "connector",
        }
    }

    /// The source a stored spelling names, or `None` for any other string.
    #[must_use]
    pub fn parse(stored: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|source| source.as_str() == stored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_source_round_trips_through_its_stored_spelling() {
        for source in Source::ALL {
            assert_eq!(Source::parse(source.as_str()), Some(source));
        }
    }

    #[test]
    fn the_stored_spellings_are_the_specs() {
        let spellings: Vec<&str> = Source::ALL.iter().map(|s| s.as_str()).collect();
        assert_eq!(spellings, ["local", "idp", "sync", "scim", "connector"]);
    }

    #[test]
    fn an_unknown_or_differently_cased_spelling_is_none() {
        for stored in ["ldap", "Local", "", " local"] {
            assert_eq!(Source::parse(stored), None, "{stored:?}");
        }
    }
}
