// SPDX-License-Identifier: AGPL-3.0-or-later
//! The database schema, as embedded migrations, and the conversions repository code shares.

use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::migrate::Migrator;

/// Every migration in `migrations/` at the repository root, compiled in. Migrations are
/// forward-only: an applied migration is never edited, and a change is a new migration.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// A time before 1970 or past what an `i64` of microseconds holds, which no clock reading of a
/// running server produces.
#[derive(Debug, thiserror::Error)]
#[error("the time is outside the range the database stores")]
pub struct TimeOutOfRange;

/// `time` as whole microseconds since the Unix epoch, PostgreSQL's `timestamptz` resolution,
/// for binding to SQL as `timestamptz 'epoch' + $n * interval '1 microsecond'`.
///
/// The workspace has no date-time crate yet, and sqlx binds `SystemTime` only through one.
/// PostgreSQL multiplies the interval by the integer as a `float8`, which holds every
/// microsecond count exactly up to 2^53 (the year 2255). Sub-microsecond parts are truncated.
pub fn epoch_micros(time: SystemTime) -> Result<i64, TimeOutOfRange> {
    let since = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| TimeOutOfRange)?;
    i64::try_from(since.as_micros()).map_err(|_| TimeOutOfRange)
}

/// The time `micros` microseconds after the Unix epoch, for reading a `timestamptz` selected as
/// `(extract(epoch FROM column) * 1000000)::bigint`. The inverse of [`epoch_micros`].
pub fn from_epoch_micros(micros: i64) -> Result<SystemTime, TimeOutOfRange> {
    let micros = u64::try_from(micros).map_err(|_| TimeOutOfRange)?;
    UNIX_EPOCH
        .checked_add(std::time::Duration::from_micros(micros))
        .ok_or(TimeOutOfRange)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn times_convert_to_whole_microseconds() {
        let time = UNIX_EPOCH + Duration::new(1_763_000_000, 123_456_789);
        assert_eq!(epoch_micros(time).unwrap(), 1_763_000_000_123_456);
        assert_eq!(epoch_micros(UNIX_EPOCH).unwrap(), 0);
    }

    #[test]
    fn microseconds_convert_back_to_the_same_time() {
        let time = UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456);
        assert_eq!(
            from_epoch_micros(epoch_micros(time).unwrap()).unwrap(),
            time
        );
        assert_eq!(from_epoch_micros(0).unwrap(), UNIX_EPOCH);
    }

    #[test]
    fn a_negative_microsecond_count_is_refused() {
        assert!(from_epoch_micros(-1).is_err());
    }

    #[test]
    fn a_time_before_the_epoch_is_refused() {
        let time = UNIX_EPOCH - Duration::from_secs(1);
        assert!(epoch_micros(time).is_err());
    }
}
