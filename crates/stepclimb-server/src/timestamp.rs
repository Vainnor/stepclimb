// SPDX-License-Identifier: AGPL-3.0-or-later
//! Timestamps in API bodies: RFC 3339 in UTC with microseconds, `2025-11-13T02:13:20.123456Z`.
//!
//! Microseconds are PostgreSQL's `timestamptz` resolution, and a fixed width keeps the strings
//! in time order when sorted as text. The workspace has no date-time crate; the date comes from
//! the day count with the proleptic Gregorian algorithm of Howard Hinnant's `civil_from_days`.

use std::time::{SystemTime, UNIX_EPOCH};

/// A time before 1970, which this formatter does not handle and no row holds, or after 9999,
/// which a four-digit year cannot write.
#[derive(Debug, thiserror::Error)]
#[error("the time cannot be written as an RFC 3339 timestamp")]
pub struct Unrepresentable;

const SECONDS_PER_DAY: u64 = 86_400;

/// `time` as RFC 3339 in UTC with six fractional digits. Sub-microsecond parts are truncated.
pub fn rfc3339(time: SystemTime) -> Result<String, Unrepresentable> {
    let since = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Unrepresentable)?;
    let seconds = since.as_secs();
    let (year, month, day) = civil_from_days(seconds / SECONDS_PER_DAY);
    if year > 9999 {
        return Err(Unrepresentable);
    }
    let of_day = seconds % SECONDS_PER_DAY;
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:06}Z",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60,
        since.subsec_micros(),
    ))
}

/// The year, month (1 to 12) and day (1 to 31) of the day `days` after 1970-01-01.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    // Shift the epoch to 0000-03-01, so each 400-year era starts on a March 1 and the leap day
    // is the last day of its year.
    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(seconds: u64, micros: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds) + Duration::from_micros(micros)
    }

    /// Values checked against `date -u -d @<seconds> +%FT%TZ` (GNU coreutils).
    #[test]
    fn known_instants_format_as_rfc3339() {
        let cases = [
            (0, 0, "1970-01-01T00:00:00.000000Z"),
            (1_763_000_000, 123_456, "2025-11-13T02:13:20.123456Z"),
            (951_782_400, 0, "2000-02-29T00:00:00.000000Z"),
            (951_868_799, 999_999, "2000-02-29T23:59:59.999999Z"),
            (4_107_542_399, 1, "2100-02-28T23:59:59.000001Z"),
            (4_107_542_400, 0, "2100-03-01T00:00:00.000000Z"),
            (1_709_164_800, 0, "2024-02-29T00:00:00.000000Z"),
            (1_735_689_599, 0, "2024-12-31T23:59:59.000000Z"),
            (253_402_300_799, 0, "9999-12-31T23:59:59.000000Z"),
        ];
        for (seconds, micros, expected) in cases {
            assert_eq!(rfc3339(at(seconds, micros)).unwrap(), expected, "{seconds}");
        }
    }

    #[test]
    fn nanoseconds_are_truncated() {
        let time = UNIX_EPOCH + Duration::new(0, 1_999);
        assert_eq!(rfc3339(time).unwrap(), "1970-01-01T00:00:00.000001Z");
    }

    #[test]
    fn every_day_follows_the_previous_one() {
        // Each day of 1970 to 2399 is the day after the one before it, so no month length or
        // leap rule is skipped.
        let mut previous = civil_from_days(0);
        assert_eq!(previous, (1970, 1, 1));
        for days in 1..157_000 {
            let next = civil_from_days(days);
            let (y, m, d) = previous;
            let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
            let month_len = match m {
                2 if leap => 29,
                2 => 28,
                4 | 6 | 9 | 11 => 30,
                _ => 31,
            };
            let expected = if d < month_len {
                (y, m, d + 1)
            } else if m < 12 {
                (y, m + 1, 1)
            } else {
                (y + 1, 1, 1)
            };
            assert_eq!(next, expected, "day {days}");
            previous = next;
        }
    }

    #[test]
    fn times_outside_four_digit_years_are_refused() {
        assert!(rfc3339(UNIX_EPOCH - Duration::from_secs(1)).is_err());
        assert!(rfc3339(at(253_402_300_800, 0)).is_err());
    }
}
