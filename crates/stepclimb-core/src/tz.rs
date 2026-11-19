// SPDX-License-Identifier: AGPL-3.0-or-later
//! IANA time zone names (docs/specs/m0/03-users-and-identities.md B3, Q4).
//!
//! A user's `tz` and the organization's `default_tz` setting are names of the IANA time zone
//! database, checked with `jiff` against the copy of the database it bundles (`jiff-tzdb`), never
//! the host's zoneinfo: the same name is accepted on every host, in the distroless image and in
//! tests, and a name is never turned into a file path. The database is updated with `jiff-tzdb`
//! in the monthly dependency update.

use std::sync::LazyLock;

use jiff::tz::{TimeZone, TimeZoneDatabase};

/// The time zone of an organization that has not set one, and so of its users (Q4).
pub const DEFAULT: &str = "UTC";

static DATABASE: LazyLock<TimeZoneDatabase> = LazyLock::new(TimeZoneDatabase::bundled);

/// The database's spelling of the time zone `name`, matched without regard to ASCII case
/// (`america/new_york` is `America/New_York`), or `None` when the database has no such zone.
/// `Etc/Unknown`, which marks an unknown zone and is no IANA name, and fixed offsets such as
/// `+05:00`, which have no IANA name, are `None`.
pub fn canonical(name: &str) -> Option<String> {
    let zone = DATABASE.get(name).ok()?;
    zone.iana_name().map(str::to_owned)
}

/// The time zone `name` from the bundled database, or `None` when it has no such zone. A stored
/// name was checked with [`canonical`] when it was written.
#[must_use]
pub fn zone(name: &str) -> Option<TimeZone> {
    DATABASE.get(name).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_of_the_database_are_accepted_in_its_spelling() {
        for (given, stored) in [
            ("UTC", "UTC"),
            ("utc", "UTC"),
            ("America/New_York", "America/New_York"),
            ("america/NEW_YORK", "America/New_York"),
            ("Europe/Dublin", "Europe/Dublin"),
            ("Asia/Kolkata", "Asia/Kolkata"),
            ("Etc/GMT+5", "Etc/GMT+5"),
        ] {
            assert_eq!(canonical(given).as_deref(), Some(stored), "{given:?}");
        }
    }

    #[test]
    fn anything_else_is_refused() {
        for name in [
            "",
            " UTC",
            "UTC ",
            "Mars/Olympus_Mons",
            "America/New York",
            "Etc/Unknown",
            "+05:00",
            "EST5EDT,M3.2.0,M11.1.0",
            "../../etc/passwd",
            "/usr/share/zoneinfo/UTC",
            "UTC\0",
        ] {
            assert_eq!(canonical(name), None, "{name:?}");
        }
    }

    #[test]
    fn a_zone_is_found_by_its_name_only() {
        assert_eq!(
            zone("Europe/Dublin").and_then(|z| z.iana_name().map(str::to_owned)),
            Some("Europe/Dublin".to_owned())
        );
        assert!(zone("Mars/Olympus_Mons").is_none());
        assert!(zone("../../etc/passwd").is_none());
    }

    #[test]
    fn the_default_is_a_name_of_the_database() {
        assert_eq!(canonical(DEFAULT).as_deref(), Some(DEFAULT));
    }
}
