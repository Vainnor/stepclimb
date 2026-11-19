// SPDX-License-Identifier: AGPL-3.0-or-later
//! Completion history (docs/specs/m1/14-recurrence-and-expiring-completions.md B1 to B3, B13).
//!
//! Every course and module completion is one `completions` row per cycle, written by
//! [`record`] in the transaction where `crate::progress` sets the module's or course's
//! `completed_at` (spec 05 B6), under the learner's `course_progress` lock, so two completions
//! of one learner in one course take turns and each reads the cycle the other wrote. `cycle`
//! counts the user's completions of the target: 1 for the first. Item completions are not
//! recorded (Q5).
//!
//! `expires_at` is `completed_at` plus the target's `completion_valid_months`, in calendar
//! months in the organization's default time zone ([`add_months`]), or null when the target has
//! no validity. The validity is read when the completion is recorded, so changing it later
//! leaves stored values as they are (Q3).
//!
//! [`list_for_course`] and [`list_for_user`] read the history newest first. Who may read whose
//! is the routes' to decide; they pass an Observer's learners as [`CourseFilter::only`].
//!
//! Every query filters by `org_id = scope.org_id()`.

use std::time::SystemTime;

use jiff::{Span, Timestamp, tz::TimeZone};
use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    id::new_id,
    organizations, tz,
};

/// The fewest months a validity may be (B3, Q2).
pub const MIN_VALID_MONTHS: i16 = 1;

/// The most months a validity may be (B3, Q2).
pub const MAX_VALID_MONTHS: i16 = 120;

/// Whether `months` is a validity B3 allows: 1 to 120.
#[must_use]
pub fn valid_months(months: i16) -> bool {
    (MIN_VALID_MONTHS..=MAX_VALID_MONTHS).contains(&months)
}

/// What a completion is of (B1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TargetType {
    Course,
    Module,
}

impl TargetType {
    /// The stored and API name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Course => "course",
            Self::Module => "module",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "course" => Some(Self::Course),
            "module" => Some(Self::Module),
            _ => None,
        }
    }
}

/// One completion of a course or a module by a user, in one cycle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub id: Uuid,
    pub user_id: Uuid,
    pub display_name: String,
    pub course_id: Uuid,
    pub target_type: TargetType,
    pub target_id: Uuid,
    /// The course's or module's current title; `None` for a module deleted since.
    pub target_title: Option<String>,
    /// 1 for the user's first completion of the target, then 2, and so on.
    pub cycle: i32,
    pub completed_at: SystemTime,
    /// `None` when the target had no validity when it was completed.
    pub expires_at: Option<SystemTime>,
    /// When the expiry job expired it (spec 14 I2); `None` until then.
    pub expired_at: Option<SystemTime>,
}

/// Why a completions call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The organization's stored default time zone is not in the bundled database, which the
    /// settings write's check keeps from happening.
    #[error("the organization's time zone {name:?} is not in the time zone database")]
    TimeZone { name: String },
    /// No course, or module, of the scope's organization with the id the caller passed: a bug
    /// in the caller, which holds the course row.
    #[error("the {0} of the completion does not exist")]
    TargetNotFound(&'static str),
    /// A list cursor whose time is before the Unix epoch, which no stored completion is.
    #[error("the cursor is not one this list wrote")]
    InvalidCursor,
    /// A stored value the table's checks keep from happening.
    #[error("the {column} of completion {id} cannot be read")]
    Stored { id: Uuid, column: &'static str },
    #[error(transparent)]
    Organization(#[from] organizations::Error),
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// `at` plus `months` calendar months on the wall clock of `zone` (B2). A day the month does not
/// have becomes its last day (31 January plus 1 month is the last day of February), and a wall
/// time a daylight-saving change skips or repeats takes jiff's compatible choice.
pub fn add_months(
    at: SystemTime,
    months: i16,
    zone: &TimeZone,
) -> Result<SystemTime, TimeOutOfRange> {
    let start = Timestamp::try_from(at).map_err(|_| TimeOutOfRange)?;
    let span = Span::new().try_months(months).map_err(|_| TimeOutOfRange)?;
    let end = start
        .to_zoned(zone.clone())
        .checked_add(span)
        .map_err(|_| TimeOutOfRange)?;
    Ok(SystemTime::from(end.timestamp()))
}

/// Record the completion of `target_type` `target_id`, of the course `course_id` of the scope's
/// organization, by the learner `user_id` at `completed_at`, as the next cycle (B1, B2). The
/// caller holds the learner's `course_progress` row `FOR UPDATE` in `conn`'s transaction, which
/// is what makes the cycle count safe; the unique key backs it.
pub(crate) async fn record(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
    course_id: Uuid,
    (target_type, target_id): (TargetType, Uuid),
    completed_at: SystemTime,
) -> Result<(), Error> {
    let validity = validity(&mut *conn, scope, course_id, target_type, target_id).await?;
    let expires_at = match validity {
        Some(months) => {
            let zone = org_zone(&mut *conn, scope).await?;
            Some(add_months(completed_at, months, &zone)?)
        }
        None => None,
    };
    sqlx::query!(
        "INSERT INTO completions
             (id, org_id, user_id, course_id, target_type, target_id, cycle, completed_at,
              expires_at, created_at, updated_at)
         SELECT $1, $2, $3, $4, $5, $6,
                coalesce((SELECT max(cycle) FROM completions
                          WHERE org_id = $2 AND user_id = $3 AND target_type = $5
                            AND target_id = $6), 0) + 1,
                timestamptz 'epoch' + $7::bigint * interval '1 microsecond',
                timestamptz 'epoch' + $8::bigint * interval '1 microsecond',
                timestamptz 'epoch' + $7::bigint * interval '1 microsecond',
                timestamptz 'epoch' + $7::bigint * interval '1 microsecond'",
        new_id(),
        scope.org_id(),
        user_id,
        course_id,
        target_type.as_str(),
        target_id,
        epoch_micros(completed_at)?,
        expires_at.map(epoch_micros).transpose()?,
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// The target's `completion_valid_months` as it is now.
async fn validity(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    target_type: TargetType,
    target_id: Uuid,
) -> Result<Option<i16>, Error> {
    let found = match target_type {
        TargetType::Course => {
            sqlx::query_scalar!(
                "SELECT completion_valid_months FROM courses WHERE org_id = $1 AND id = $2",
                scope.org_id(),
                target_id,
            )
            .fetch_optional(conn)
            .await?
        }
        TargetType::Module => {
            sqlx::query_scalar!(
                "SELECT completion_valid_months FROM modules
                 WHERE org_id = $1 AND course_id = $2 AND id = $3",
                scope.org_id(),
                course_id,
                target_id,
            )
            .fetch_optional(conn)
            .await?
        }
    };
    found.ok_or(Error::TargetNotFound(target_type.as_str()))
}

/// The organization's default time zone (B2).
async fn org_zone(conn: &mut PgConnection, scope: OrgScope) -> Result<TimeZone, Error> {
    let name = organizations::settings(conn, scope).await?.default_tz;
    tz::zone(&name).ok_or(Error::TimeZone { name })
}

/// The sort key of a completion in a list: `completed_at` in microseconds since the Unix epoch,
/// then the id. Lists run newest first.
pub type ListKey = (i64, Uuid);

/// The filters of [`list_for_course`]; `None` filters nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CourseFilter {
    pub user_id: Option<Uuid>,
    pub target_type: Option<TargetType>,
    /// Only these users: an Observer's observed learners (spec 05 B20).
    pub only: Option<Vec<Uuid>>,
    /// Leave out completions of modules that are unpublished or deleted: a staff read without
    /// `course.view_all`, which never names unpublished content (spec 05 B14).
    pub published_only: bool,
}

/// The completions of the course `course_id` of the scope's organization and its modules
/// matching `filter`, newest first, after `after` (the key of the previous page's last row) and
/// at most `limit` of them (B13). `after` comes from a client's cursor and only bounds a query
/// already filtered by the scope; a time before the Unix epoch is [`Error::InvalidCursor`].
pub async fn list_for_course(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    course_id: Uuid,
    filter: &CourseFilter,
    after: Option<&ListKey>,
    limit: i64,
) -> Result<Vec<Completion>, Error> {
    let query = Query {
        course_id: Some(course_id),
        user_id: filter.user_id,
        target_type: filter.target_type,
        only: filter.only.as_deref(),
        published_only: filter.published_only,
    };
    list(db, scope, &query, after, limit).await
}

/// The user `user_id`'s own completions in the scope's organization, of the course `course_id`
/// and its modules when given, newest first, after `after` and at most `limit` of them (B13).
pub async fn list_for_user(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: Uuid,
    course_id: Option<Uuid>,
    after: Option<&ListKey>,
    limit: i64,
) -> Result<Vec<Completion>, Error> {
    let query = Query {
        course_id,
        user_id: Some(user_id),
        target_type: None,
        only: None,
        published_only: false,
    };
    list(db, scope, &query, after, limit).await
}

/// What [`list`] selects.
struct Query<'a> {
    course_id: Option<Uuid>,
    user_id: Option<Uuid>,
    target_type: Option<TargetType>,
    only: Option<&'a [Uuid]>,
    published_only: bool,
}

struct Row {
    id: Uuid,
    user_id: Uuid,
    display_name: String,
    course_id: Uuid,
    target_type: String,
    target_id: Uuid,
    target_title: Option<String>,
    cycle: i32,
    completed_at_us: i64,
    expires_at_us: Option<i64>,
    expired_at_us: Option<i64>,
}

impl TryFrom<Row> for Completion {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        Ok(Self {
            target_type: TargetType::parse(&row.target_type).ok_or(Error::Stored {
                id: row.id,
                column: "target_type",
            })?,
            completed_at: from_epoch_micros(row.completed_at_us)?,
            expires_at: row.expires_at_us.map(from_epoch_micros).transpose()?,
            expired_at: row.expired_at_us.map(from_epoch_micros).transpose()?,
            id: row.id,
            user_id: row.user_id,
            display_name: row.display_name,
            course_id: row.course_id,
            target_id: row.target_id,
            target_title: row.target_title,
            cycle: row.cycle,
        })
    }
}

async fn list(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    query: &Query<'_>,
    after: Option<&ListKey>,
    limit: i64,
) -> Result<Vec<Completion>, Error> {
    if let Some((at, _)) = after {
        from_epoch_micros(*at).map_err(|_| Error::InvalidCursor)?;
    }
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT c.id, c.user_id, u.display_name, c.course_id, c.target_type, c.target_id,
                  CASE c.target_type WHEN 'course' THEN co.title ELSE m.title END AS target_title,
                  c.cycle,
                  (extract(epoch FROM c.completed_at) * 1000000)::bigint AS "completed_at_us!",
                  (extract(epoch FROM c.expires_at) * 1000000)::bigint AS expires_at_us,
                  (extract(epoch FROM c.expired_at) * 1000000)::bigint AS expired_at_us
           FROM completions c
           JOIN users u ON u.org_id = c.org_id AND u.id = c.user_id
           JOIN courses co ON co.org_id = c.org_id AND co.id = c.course_id
           LEFT JOIN modules m ON c.target_type = 'module'
                AND m.org_id = c.org_id AND m.course_id = c.course_id AND m.id = c.target_id
           WHERE c.org_id = $1
             AND ($2::uuid IS NULL OR c.course_id = $2)
             AND ($3::uuid IS NULL OR c.user_id = $3)
             AND ($4::text IS NULL OR c.target_type = $4)
             AND ($5::uuid[] IS NULL OR c.user_id = ANY($5))
             AND (NOT $6 OR c.target_type = 'course' OR m.published)
             AND ($7::bigint IS NULL
                  OR (c.completed_at, c.id)
                     < (timestamptz 'epoch' + $7::bigint * interval '1 microsecond', $8::uuid))
           ORDER BY c.completed_at DESC, c.id DESC
           LIMIT $9"#,
        scope.org_id(),
        query.course_id,
        query.user_id,
        query.target_type.map(TargetType::as_str),
        query.only,
        query.published_only,
        after.map(|(at, _)| *at),
        after.map(|(_, id)| *id),
        limit,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(Completion::try_from).collect()
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    fn at(rfc3339: &str) -> SystemTime {
        SystemTime::from(rfc3339.parse::<Timestamp>().expect("a timestamp"))
    }

    fn zone(name: &str) -> TimeZone {
        tz::zone(name).expect("a zone of the database")
    }

    #[test]
    fn t1_months_are_calendar_months_and_a_missing_day_is_the_months_last() {
        let utc = zone("UTC");
        for (start, months, end) in [
            ("2026-03-10T12:00:00Z", 12, "2027-03-10T12:00:00Z"),
            ("2026-01-31T12:00:00Z", 1, "2026-02-28T12:00:00Z"),
            ("2028-01-31T12:00:00Z", 1, "2028-02-29T12:00:00Z"),
            ("2026-08-31T00:00:00Z", 6, "2027-02-28T00:00:00Z"),
            ("2026-01-15T00:00:00Z", 120, "2036-01-15T00:00:00Z"),
        ] {
            assert_eq!(
                add_months(at(start), months, &utc).unwrap(),
                at(end),
                "{start} + {months}"
            );
        }
    }

    #[test]
    fn t1_months_are_counted_on_the_organizations_wall_clock() {
        // 03:00 UTC on 31 January is 22:00 on 30 January in New York: one month later is
        // 22:00 on 28 February there, 03:00 UTC on 1 March, where UTC's own count stops on
        // 28 February.
        let start = at("2026-01-31T03:00:00Z");
        assert_eq!(
            add_months(start, 1, &zone("America/New_York")).unwrap(),
            at("2026-03-01T03:00:00Z")
        );
        assert_eq!(
            add_months(start, 1, &zone("UTC")).unwrap(),
            at("2026-02-28T03:00:00Z")
        );
        // Across a daylight-saving change the wall time stays: 09:00 in Dublin in winter
        // (UTC) and summer (UTC+1).
        assert_eq!(
            add_months(at("2026-01-10T09:00:00Z"), 6, &zone("Europe/Dublin")).unwrap(),
            at("2026-07-10T08:00:00Z")
        );
    }

    #[test]
    fn sub_second_parts_are_kept() {
        let start = UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456);
        let end = add_months(start, 1, &zone("UTC")).unwrap();
        assert_eq!(epoch_micros(end).unwrap() % 1_000_000, 123_456);
    }

    #[test]
    fn validity_is_1_to_120_months() {
        for months in [1, 12, 120] {
            assert!(valid_months(months), "{months}");
        }
        for months in [i16::MIN, -1, 0, 121, i16::MAX] {
            assert!(!valid_months(months), "{months}");
        }
    }

    #[test]
    fn target_types_round_trip_their_names() {
        for target in [TargetType::Course, TargetType::Module] {
            assert_eq!(TargetType::parse(target.as_str()), Some(target));
        }
        assert_eq!(TargetType::parse("item"), None);
    }
}
