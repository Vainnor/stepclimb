// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pure parts of an assignment: when a person's due date falls (B4) and what their row's
//! status is (B7). No I/O; the repository reads the times and calls these.

use std::time::SystemTime;

use jiff::{Timestamp, ToSpan, tz::TimeZone};

use crate::db::TimeOutOfRange;

/// The most days a relative due rule may count (B4).
pub const MAX_RELATIVE_DAYS: u16 = 3650;

/// When an assignment is due for each person it reaches (B4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DueRule {
    /// The same time for everyone.
    Fixed(SystemTime),
    /// 1 to [`MAX_RELATIVE_DAYS`] calendar days after the person was assigned, at 23:59:59
    /// local time of the last day in the organization's default time zone (Q1).
    RelativeDays(u16),
}

impl DueRule {
    /// The due time of a person assigned at `assigned_at`, in an organization whose default
    /// time zone is `zone`. A relative rule's last day is the local date of `assigned_at` plus
    /// its days, so a change of offset in between (daylight saving time) still ends the day at
    /// 23:59:59 local; a local time that a change skips or repeats resolves as jiff's
    /// `compatible` rule does (the later instant of a gap, the earlier of a fold).
    pub fn due_at(self, assigned_at: SystemTime, zone: &TimeZone) -> Result<SystemTime, DueError> {
        let days = match self {
            Self::Fixed(at) => return Ok(at),
            Self::RelativeDays(days) => days,
        };
        if !(1..=MAX_RELATIVE_DAYS).contains(&days) {
            return Err(DueError::DaysOutOfRange);
        }
        let assigned = Timestamp::try_from(assigned_at).map_err(|_| TimeOutOfRange)?;
        let last_day = assigned
            .to_zoned(zone.clone())
            .date()
            .checked_add(i64::from(days).days())
            .map_err(|_| TimeOutOfRange)?;
        let end = last_day
            .at(23, 59, 59, 0)
            .to_zoned(zone.clone())
            .map_err(|_| TimeOutOfRange)?;
        Ok(SystemTime::from(end.timestamp()))
    }
}

/// Why [`DueRule::due_at`] gave no time.
#[derive(Debug, thiserror::Error)]
pub enum DueError {
    #[error("a relative due rule counts 1 to {MAX_RELATIVE_DAYS} days")]
    DaysOutOfRange,
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
}

/// A person's status on an assignment (B7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    NotStarted,
    InProgress,
    Completed,
    Overdue,
}

impl Status {
    /// The API name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Overdue => "overdue",
        }
    }
}

/// What B7 reads of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowState {
    pub due_at: Option<SystemTime>,
    pub completed_at: Option<SystemTime>,
    /// Whether the person has any progress on the target (spec 05 B3).
    pub has_progress: bool,
}

impl RowState {
    /// B7 at `now`: `completed` once `completed_at` is set, late or not (B9); else `overdue`
    /// once `due_at` has passed; else `in_progress` with any progress on the target; else
    /// `not_started`. A row without a due date is never overdue (B4).
    #[must_use]
    pub fn status(&self, now: SystemTime) -> Status {
        if self.completed_at.is_some() {
            Status::Completed
        } else if self.due_at.is_some_and(|due| due < now) {
            Status::Overdue
        } else if self.has_progress {
            Status::InProgress
        } else {
            Status::NotStarted
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(text: &str) -> SystemTime {
        SystemTime::from(text.parse::<Timestamp>().unwrap())
    }

    fn zone(name: &str) -> TimeZone {
        crate::tz::zone(name).unwrap()
    }

    /// T2: 14 days from 2026-03-01 10:00 UTC in New York is 2026-03-15 23:59:59 local. The
    /// clocks go forward on 2026-03-08, so the day ends at UTC-4, not the UTC-5 of the start.
    #[test]
    fn a_relative_due_date_ends_its_last_local_day_across_a_dst_change() {
        let due = DueRule::RelativeDays(14)
            .due_at(at("2026-03-01T10:00:00Z"), &zone("America/New_York"))
            .unwrap();
        assert_eq!(due, at("2026-03-16T03:59:59Z"));
    }

    #[test]
    fn a_relative_due_date_counts_from_the_local_date_of_assignment() {
        // 03:00 UTC on 2 March is still 1 March in New York (UTC-5).
        let due = DueRule::RelativeDays(1)
            .due_at(at("2026-03-02T03:00:00Z"), &zone("America/New_York"))
            .unwrap();
        assert_eq!(due, at("2026-03-03T04:59:59Z"));
        let utc = DueRule::RelativeDays(1)
            .due_at(at("2026-03-02T03:00:00Z"), &zone("UTC"))
            .unwrap();
        assert_eq!(utc, at("2026-03-03T23:59:59Z"));
    }

    #[test]
    fn a_fixed_due_date_is_kept_as_given() {
        let fixed = at("2026-05-01T12:34:56Z");
        assert_eq!(
            DueRule::Fixed(fixed)
                .due_at(at("2026-03-01T00:00:00Z"), &zone("Asia/Kolkata"))
                .unwrap(),
            fixed
        );
    }

    #[test]
    fn relative_days_outside_1_to_3650_are_refused() {
        let start = at("2026-03-01T00:00:00Z");
        for days in [0, MAX_RELATIVE_DAYS + 1] {
            assert!(matches!(
                DueRule::RelativeDays(days).due_at(start, &zone("UTC")),
                Err(DueError::DaysOutOfRange)
            ));
        }
        assert!(
            DueRule::RelativeDays(MAX_RELATIVE_DAYS)
                .due_at(start, &zone("UTC"))
                .is_ok()
        );
    }

    #[test]
    fn the_status_follows_b7_in_order() {
        let now = at("2026-03-10T00:00:00Z");
        let past = now - Duration::from_secs(1);
        let future = now + Duration::from_secs(1);
        let row = |due_at, completed_at, has_progress| RowState {
            due_at,
            completed_at,
            has_progress,
        };
        assert_eq!(row(None, None, false).status(now), Status::NotStarted);
        assert_eq!(
            row(Some(future), None, true).status(now),
            Status::InProgress
        );
        assert_eq!(row(Some(past), None, true).status(now), Status::Overdue);
        assert_eq!(row(Some(now), None, false).status(now), Status::NotStarted);
        assert_eq!(
            row(Some(past), Some(now), false).status(now),
            Status::Completed,
            "completed late is completed (B9)"
        );
        assert_eq!(row(None, None, true).status(now), Status::InProgress);
    }
}
