// SPDX-License-Identifier: AGPL-3.0-or-later
//! Assignments to users with due dates
//! (docs/specs/m1/13-assignments-due-dates-reminders-and-escalation.md B1 to B5, B7 to B9).
//!
//! An assignment names what must be finished (a course, or a module or item of it) and who must
//! finish it, with an optional [`DueRule`]. Each person it reaches gets an `assignment_users`
//! row with their own `assigned_at`, `due_at` and `completed_at`; a user assignee is one row,
//! made with the assignment. Group assignees are spec 13 I5.
//!
//! [`create`] locks the course row `FOR NO KEY UPDATE`, as every content and enrollment write
//! does, so the target cannot be deleted, the course cannot be archived and a progress write
//! (which reads the row `FOR SHARE`) cannot complete the target between the checks and the
//! insert. It refuses an archived course or a template (B3), and a target that is not the
//! course's. An assignee who is not a learner of the course is enrolled as Student only when the
//! caller says it may (B3, Q2): the route decides that from `enrollment.manage`. A target the
//! person already completed completes the row at once (B8, Q5).
//!
//! Completion afterwards arrives through the progress events (spec 05 B10):
//! [`record_completion`] sets `completed_at` on the rows of not-yet-completed cycle-1
//! assignments of that target, and only those, so a rerun changes nothing (spec m1/01 B12).
//!
//! Statuses (B7) are computed when read, from the stored times, the person's progress and the
//! caller's clock, by [`RowState::status`]; [`Assignment::counts`] totals them.
//!
//! Every query filters by `org_id = scope.org_id()`.

mod rules;

use std::time::SystemTime;

use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};
use uuid::Uuid;

pub use rules::{DueError, DueRule, MAX_RELATIVE_DAYS, RowState, Status};

use crate::{
    OrgScope, Source,
    audit::Actor,
    courses,
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    enrollments::{self, NewEnrollment},
    id::new_id,
    organizations, roles, tz,
};

/// The built-in role an assignment enrolls a person with (B3).
pub const STUDENT_ROLE: &str = "student";

/// What an assignment names (B1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TargetType {
    Course,
    Module,
    Item,
}

impl TargetType {
    /// The stored and API name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Course => "course",
            Self::Module => "module",
            Self::Item => "item",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "course" => Some(Self::Course),
            "module" => Some(Self::Module),
            "item" => Some(Self::Item),
            _ => None,
        }
    }
}

/// How many of an assignment's rows are in each status (B7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub not_started: u32,
    pub in_progress: u32,
    pub completed: u32,
    pub overdue: u32,
}

impl Counts {
    fn add(&mut self, status: Status) {
        let count = match status {
            Status::NotStarted => &mut self.not_started,
            Status::InProgress => &mut self.in_progress,
            Status::Completed => &mut self.completed,
            Status::Overdue => &mut self.overdue,
        };
        *count += 1;
    }
}

/// An assignment as the repository reads it, with its rows' statuses at the reader's `now`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assignment {
    pub id: Uuid,
    pub course_id: Uuid,
    pub target_type: TargetType,
    pub target_id: Uuid,
    pub target_title: String,
    /// The assigned user.
    pub user_id: Uuid,
    pub user_display_name: String,
    pub due: Option<DueRule>,
    pub created_by: Uuid,
    pub ended_at: Option<SystemTime>,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
    pub counts: Counts,
}

/// An assignment to create for one user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewAssignment {
    pub target_type: TargetType,
    pub target_id: Uuid,
    pub user_id: Uuid,
    pub due: Option<DueRule>,
    /// The user who creates it.
    pub created_by: Uuid,
}

/// Whether [`create`] may enroll an assignee who is not a learner of the course (B3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enroll {
    /// Refuse with [`Error::EnrollmentNeeded`].
    Refuse,
    /// Enroll them as Student, `local`, written by `actor`.
    AsStudent(Actor),
}

/// What became complete for a learner, from a progress event (B8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Completed {
    pub user_id: Uuid,
    pub target_type: TargetType,
    pub target_id: Uuid,
    pub completed_at: SystemTime,
}

/// Why an assignments repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No assignment with that id in the scope's organization.
    #[error("the assignment does not exist")]
    NotFound,
    /// No course with that id in the scope's organization.
    #[error("the course does not exist")]
    CourseNotFound,
    /// The course is archived and takes no new assignment (B3).
    #[error("an archived course takes no assignment")]
    CourseArchived,
    /// The course is a template and takes no assignment (B3).
    #[error("a template takes no assignment")]
    CourseTemplate,
    /// The target is not the course, or no module or item of it.
    #[error("the {} does not exist in the course", .0.as_str())]
    TargetNotFound(TargetType),
    /// No user with that id in the scope's organization.
    #[error("the user does not exist")]
    UserNotFound,
    /// The assignee is not a learner of the course and the caller may not enroll them (B3).
    #[error("the user is not a learner of the course")]
    EnrollmentNeeded,
    /// The assignee's Student enrollment in the course is concluded and owned by an outside
    /// source, so the assignment cannot reactivate it.
    #[error("an outside source owns the user's Student enrollment")]
    EnrollmentExternallyManaged,
    /// The built-in Student role has no row: `roles::sync_builtin` has not run.
    #[error("the built-in Student role is missing")]
    StudentRoleMissing,
    /// The assignment has ended and takes no change.
    #[error("the assignment has ended")]
    Ended,
    /// The due rule cannot give a due time.
    #[error(transparent)]
    Due(#[from] DueError),
    /// The organization's stored default time zone is not one the bundled database knows.
    #[error("the organization's time zone {0:?} is unknown")]
    UnknownTimeZone(String),
    /// A stored value this release does not know.
    #[error("the stored {column} {value:?} is not one this release knows")]
    UnknownStoredValue { column: &'static str, value: String },
    #[error(transparent)]
    Enrollment(#[from] enrollments::Error),
    #[error(transparent)]
    Course(#[from] courses::Error),
    #[error(transparent)]
    Organization(#[from] organizations::Error),
    #[error(transparent)]
    Role(#[from] roles::Error),
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// The course of the assignment `id` of the scope's organization, or `None` when it has no
/// such assignment: what `AssignmentTarget` checks the caller's permission at.
pub async fn course_of(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Uuid>, Error> {
    let course = sqlx::query_scalar!(
        "SELECT course_id FROM assignments WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?;
    Ok(course)
}

/// Whether the user `user_id` is a learner of the course `course_id` (an active enrollment
/// whose role holds `course.participate`, spec 05 B1), so that assigning them enrolls no one.
/// For the route, in its transaction after it locked the course row.
pub async fn is_learner(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    user_id: Uuid,
) -> Result<bool, Error> {
    Ok(enrollments::is_learner(conn, scope, course_id, user_id).await?)
}

/// Create an assignment of the course `course_id` for one user at `now` (B1 to B4, B8), its
/// row with `assigned_at = now`, and when needed the user's Student enrollment (`enroll`, B3).
/// Returns it as read at `now`.
///
/// Runs in one transaction (a savepoint when `db` is already one) under the course lock.
pub async fn create<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    course_id: Uuid,
    new: &NewAssignment,
    enroll: Enroll,
    now: SystemTime,
) -> Result<Assignment, Error> {
    let mut tx = db.begin().await?;
    let course = courses::lock(&mut tx, scope, course_id)
        .await?
        .ok_or(Error::CourseNotFound)?;
    if course.status == courses::Status::Archived {
        return Err(Error::CourseArchived);
    }
    if course.template {
        return Err(Error::CourseTemplate);
    }
    check_target(&mut tx, scope, course_id, new.target_type, new.target_id).await?;
    check_user(&mut tx, scope, new.user_id).await?;
    if !enrollments::is_learner(&mut tx, scope, course_id, new.user_id).await? {
        match enroll {
            Enroll::Refuse => return Err(Error::EnrollmentNeeded),
            Enroll::AsStudent(actor) => {
                enroll_as_student(&mut tx, scope, course_id, new.user_id, now, actor).await?;
            }
        }
    }
    let zone = org_zone(&mut tx, scope).await?;
    let due_at = new.due.map(|due| due.due_at(now, &zone)).transpose()?;
    let completed_at =
        completion_of(&mut tx, scope, new.user_id, new.target_type, new.target_id).await?;
    let id = new_id();
    insert(&mut tx, scope, id, course_id, new, now).await?;
    let row = RowTimes {
        assigned_at: now,
        due_at,
        completed_at,
    };
    insert_row(&mut tx, scope, id, new.user_id, row).await?;
    let created = get(&mut tx, scope, id, now).await?.ok_or(Error::NotFound)?;
    tx.commit().await?;
    Ok(created)
}

/// The assignment `id` of the scope's organization as read at `now`, or `None` when it has no
/// such assignment.
pub async fn get(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
    now: SystemTime,
) -> Result<Option<Assignment>, Error> {
    let rows = select(conn, scope, Select::One(id)).await?;
    let mut read = with_counts(conn, scope, rows, now).await?;
    Ok(read.pop())
}

/// The assignments of the course `course_id`, newest first, after the id `after` when given, at
/// most `limit`; ended ones only when `include_ended`. Read at `now`.
pub async fn list_for_course(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    include_ended: bool,
    after: Option<Uuid>,
    limit: i64,
    now: SystemTime,
) -> Result<Vec<Assignment>, Error> {
    let page = Page {
        course_id,
        include_ended,
        after,
        limit,
    };
    let rows = select(conn, scope, Select::Course(page)).await?;
    with_counts(conn, scope, rows, now).await
}

/// Replace the due rule of the assignment `id` with `due` (`None` clears it) and recompute
/// `due_at` of every row not completed (B5). Completed rows keep theirs, so a late completion
/// stays late. An ended assignment is [`Error::Ended`]. Returns it as read at `now`.
///
/// Runs in one transaction (a savepoint when `db` is already one) with the assignment row
/// locked.
pub async fn set_due<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    due: Option<DueRule>,
    now: SystemTime,
) -> Result<Assignment, Error> {
    let mut tx = db.begin().await?;
    lock_open(&mut tx, scope, id).await?;
    let (fixed, relative) = stored_due(due)?;
    let now_us = epoch_micros(now)?;
    sqlx::query!(
        "UPDATE assignments
         SET due_fixed = timestamptz 'epoch' + $3::bigint * interval '1 microsecond',
             due_relative = $4,
             updated_at = timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
        fixed,
        relative,
        now_us,
    )
    .execute(&mut *tx)
    .await?;
    let zone = org_zone(&mut tx, scope).await?;
    let open = sqlx::query!(
        r#"SELECT id, (extract(epoch FROM assigned_at) * 1000000)::bigint AS "assigned_at_us!"
           FROM assignment_users
           WHERE org_id = $1 AND assignment_id = $2 AND completed_at IS NULL
           ORDER BY id
           FOR NO KEY UPDATE"#,
        scope.org_id(),
        id,
    )
    .fetch_all(&mut *tx)
    .await?;
    for row in open {
        let assigned_at = from_epoch_micros(row.assigned_at_us)?;
        let due_at = due.map(|due| due.due_at(assigned_at, &zone)).transpose()?;
        let due_us = due_at.map(epoch_micros).transpose()?;
        sqlx::query!(
            "UPDATE assignment_users
             SET due_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond',
                 updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
             WHERE org_id = $1 AND id = $2",
            scope.org_id(),
            row.id,
            due_us,
            now_us,
        )
        .execute(&mut *tx)
        .await?;
    }
    let changed = get(&mut tx, scope, id, now).await?.ok_or(Error::NotFound)?;
    tx.commit().await?;
    Ok(changed)
}

/// End the assignment `id` at `now` (B5, Q3): its rows stay. Ending an ended assignment changes
/// nothing. Returns it as read at `now`.
pub async fn end<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    now: SystemTime,
) -> Result<Assignment, Error> {
    let mut tx = db.begin().await?;
    let now_us = epoch_micros(now)?;
    sqlx::query!(
        "UPDATE assignments
         SET ended_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond',
             updated_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2 AND ended_at IS NULL",
        scope.org_id(),
        id,
        now_us,
    )
    .execute(&mut *tx)
    .await?;
    let ended = get(&mut tx, scope, id, now).await?.ok_or(Error::NotFound)?;
    tx.commit().await?;
    Ok(ended)
}

/// Set `completed_at` on the rows of the user that `completed` names, in cycle 1 of every
/// assignment of its target, ended or not, that are not complete yet (B8, Q5). Returns how many
/// rows it completed; a second call with the same completion completes none. `now` is the
/// rows' `updated_at`.
pub async fn record_completion(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    completed: &Completed,
    now: SystemTime,
) -> Result<u64, Error> {
    let at_us = epoch_micros(completed.completed_at)?;
    let now_us = epoch_micros(now)?;
    let done = sqlx::query!(
        "UPDATE assignment_users au
         SET completed_at = timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
             updated_at = timestamptz 'epoch' + $6::bigint * interval '1 microsecond'
         FROM assignments a
         WHERE au.org_id = $1 AND au.user_id = $2 AND au.cycle = 1
           AND au.completed_at IS NULL
           AND a.org_id = au.org_id AND a.id = au.assignment_id
           AND a.target_type = $3 AND a.target_id = $4",
        scope.org_id(),
        completed.user_id,
        completed.target_type.as_str(),
        completed.target_id,
        at_us,
        now_us,
    )
    .execute(db)
    .await?;
    Ok(done.rows_affected())
}

/// Whether any assignment of the scope's organization, ended or not, names the module or item
/// `target_id`: deleting it is refused (Data). For the modules and items repositories, under
/// the course lock that [`create`] also takes.
pub(crate) async fn targets(
    conn: &mut PgConnection,
    scope: OrgScope,
    target_type: TargetType,
    target_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM assignments
                          WHERE org_id = $1 AND target_type = $2 AND target_id = $3)
               AS "exists!""#,
        scope.org_id(),
        target_type.as_str(),
        target_id,
    )
    .fetch_one(conn)
    .await
}

/// The due rule as its two columns.
fn stored_due(due: Option<DueRule>) -> Result<(Option<i64>, Option<i32>), Error> {
    Ok(match due {
        None => (None, None),
        Some(DueRule::Fixed(at)) => (Some(epoch_micros(at)?), None),
        Some(DueRule::RelativeDays(days)) => {
            if !(1..=MAX_RELATIVE_DAYS).contains(&days) {
                return Err(DueError::DaysOutOfRange.into());
            }
            (None, Some(i32::from(days)))
        }
    })
}

/// Lock the assignment `id` `FOR NO KEY UPDATE`: [`Error::NotFound`] when there is none,
/// [`Error::Ended`] when it has ended.
async fn lock_open(conn: &mut PgConnection, scope: OrgScope, id: Uuid) -> Result<(), Error> {
    let ended = sqlx::query_scalar!(
        r#"SELECT ended_at IS NOT NULL AS "ended!" FROM assignments
           WHERE org_id = $1 AND id = $2 FOR NO KEY UPDATE"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(conn)
    .await?
    .ok_or(Error::NotFound)?;
    if ended {
        return Err(Error::Ended);
    }
    Ok(())
}

/// The organization's default time zone (spec m0/03 B3).
async fn org_zone(conn: &mut PgConnection, scope: OrgScope) -> Result<jiff::tz::TimeZone, Error> {
    let name = organizations::settings(&mut *conn, scope).await?.default_tz;
    tz::zone(&name).ok_or(Error::UnknownTimeZone(name))
}

/// [`Error::TargetNotFound`] unless the target is the course `course_id` itself, or a module
/// or item of it.
async fn check_target(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    target_type: TargetType,
    target_id: Uuid,
) -> Result<(), Error> {
    let found = match target_type {
        TargetType::Course => target_id == course_id,
        TargetType::Module => {
            sqlx::query_scalar!(
                r#"SELECT EXISTS (SELECT 1 FROM modules
                              WHERE org_id = $1 AND course_id = $2 AND id = $3) AS "exists!""#,
                scope.org_id(),
                course_id,
                target_id,
            )
            .fetch_one(&mut *conn)
            .await?
        }
        TargetType::Item => {
            sqlx::query_scalar!(
                r#"SELECT EXISTS (SELECT 1 FROM module_items
                              WHERE org_id = $1 AND course_id = $2 AND id = $3) AS "exists!""#,
                scope.org_id(),
                course_id,
                target_id,
            )
            .fetch_one(&mut *conn)
            .await?
        }
    };
    if found {
        Ok(())
    } else {
        Err(Error::TargetNotFound(target_type))
    }
}

/// [`Error::UserNotFound`] unless the scope's organization has the user `user_id`. The row is
/// locked `FOR KEY SHARE`, so it cannot be deleted before the commit.
async fn check_user(conn: &mut PgConnection, scope: OrgScope, user_id: Uuid) -> Result<(), Error> {
    sqlx::query_scalar!(
        "SELECT id FROM users WHERE org_id = $1 AND id = $2 FOR KEY SHARE",
        scope.org_id(),
        user_id,
    )
    .fetch_optional(conn)
    .await?
    .ok_or(Error::UserNotFound)
    .map(|_| ())
}

/// Make the user a learner of the course as Student (B3): reactivate their concluded `local`
/// Student enrollment, or create one. A concluded Student enrollment an outside source owns is
/// [`Error::EnrollmentExternallyManaged`]. Both write the enrollment's audit entry as `actor`.
async fn enroll_as_student(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    user_id: Uuid,
    now: SystemTime,
    actor: Actor,
) -> Result<(), Error> {
    let student = roles::builtin_id(&mut *conn, STUDENT_ROLE)
        .await?
        .ok_or(Error::StudentRoleMissing)?;
    let existing = sqlx::query!(
        "SELECT id, source FROM enrollments
         WHERE org_id = $1 AND course_id = $2 AND user_id = $3 AND role_id = $4",
        scope.org_id(),
        course_id,
        user_id,
        student,
    )
    .fetch_optional(&mut *conn)
    .await?;
    match existing {
        Some(row) if row.source != Source::Local.as_str() => {
            Err(Error::EnrollmentExternallyManaged)
        }
        Some(row) => {
            let status = enrollments::Status::Active;
            enrollments::set_status(&mut *conn, scope, row.id, status, now, actor).await?;
            Ok(())
        }
        None => {
            let new = NewEnrollment {
                user_id,
                role_id: student,
                source: Source::Local,
                external_id: None,
                observed_user_ids: Vec::new(),
            };
            enrollments::create(&mut *conn, scope, course_id, &new, now, actor).await?;
            Ok(())
        }
    }
}

/// When the user completed the target, if they have (B8): `item_progress`, `module_progress`
/// or `course_progress`.
async fn completion_of(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
    target_type: TargetType,
    target_id: Uuid,
) -> Result<Option<SystemTime>, Error> {
    let micros = sqlx::query_scalar!(
        r#"SELECT (extract(epoch FROM completed_at) * 1000000)::bigint AS "at!"
           FROM (SELECT completed_at FROM item_progress
                 WHERE org_id = $1 AND user_id = $2 AND $3 = 'item' AND item_id = $4
                 UNION ALL
                 SELECT completed_at FROM module_progress
                 WHERE org_id = $1 AND user_id = $2 AND $3 = 'module' AND module_id = $4
                 UNION ALL
                 SELECT completed_at FROM course_progress
                 WHERE org_id = $1 AND user_id = $2 AND $3 = 'course' AND course_id = $4) p
           WHERE completed_at IS NOT NULL"#,
        scope.org_id(),
        user_id,
        target_type.as_str(),
        target_id,
    )
    .fetch_optional(conn)
    .await?;
    Ok(micros.map(from_epoch_micros).transpose()?)
}

async fn insert(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
    course_id: Uuid,
    new: &NewAssignment,
    now: SystemTime,
) -> Result<(), Error> {
    let (fixed, relative) = stored_due(new.due)?;
    sqlx::query!(
        "INSERT INTO assignments (id, org_id, course_id, target_type, target_id, assignee_type,
                                  assignee_id, due_fixed, due_relative, created_by, created_at,
                                  updated_at)
         VALUES ($1, $2, $3, $4, $5, 'user', $6,
                 timestamptz 'epoch' + $7::bigint * interval '1 microsecond', $8, $9,
                 timestamptz 'epoch' + $10::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $10::bigint * interval '1 microsecond')",
        id,
        scope.org_id(),
        course_id,
        new.target_type.as_str(),
        new.target_id,
        new.user_id,
        fixed,
        relative,
        new.created_by,
        epoch_micros(now)?,
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// The times of a new row.
struct RowTimes {
    assigned_at: SystemTime,
    due_at: Option<SystemTime>,
    completed_at: Option<SystemTime>,
}

async fn insert_row(
    conn: &mut PgConnection,
    scope: OrgScope,
    assignment_id: Uuid,
    user_id: Uuid,
    times: RowTimes,
) -> Result<(), Error> {
    let assigned_us = epoch_micros(times.assigned_at)?;
    sqlx::query!(
        "INSERT INTO assignment_users (id, org_id, assignment_id, user_id, assigned_at, due_at,
                                       completed_at, created_at, updated_at)
         VALUES ($1, $2, $3, $4,
                 timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $6::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $7::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $5::bigint * interval '1 microsecond')",
        new_id(),
        scope.org_id(),
        assignment_id,
        user_id,
        assigned_us,
        times.due_at.map(epoch_micros).transpose()?,
        times.completed_at.map(epoch_micros).transpose()?,
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// One page of a course's assignments.
#[derive(Clone, Copy)]
struct Page {
    course_id: Uuid,
    include_ended: bool,
    after: Option<Uuid>,
    limit: i64,
}

/// Which assignments [`select`] reads.
#[derive(Clone, Copy)]
enum Select {
    One(Uuid),
    Course(Page),
}

/// An `assignments` row with its target's title and its user's name, as [`select`] reads it.
struct Row {
    id: Uuid,
    course_id: Uuid,
    target_type: String,
    target_id: Uuid,
    target_title: String,
    user_id: Uuid,
    display_name: String,
    due_fixed_us: Option<i64>,
    due_relative: Option<i32>,
    created_by: Uuid,
    ended_at_us: Option<i64>,
    created_at_us: i64,
    updated_at_us: i64,
}

/// The assignments `which` names, newest first. User assignees only: the join to `users` reads
/// `assignee_id` as a user, which every row written today is.
async fn select(
    conn: &mut PgConnection,
    scope: OrgScope,
    which: Select,
) -> Result<Vec<Row>, Error> {
    let (one, page) = match which {
        Select::One(id) => (Some(id), None),
        Select::Course(page) => (None, Some(page)),
    };
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT a.id, a.course_id, a.target_type, a.target_id,
                  COALESCE(c.title, m.title, mi.title) AS "target_title!",
                  u.id AS user_id, u.display_name,
                  (extract(epoch FROM a.due_fixed) * 1000000)::bigint AS due_fixed_us,
                  a.due_relative, a.created_by,
                  (extract(epoch FROM a.ended_at) * 1000000)::bigint AS ended_at_us,
                  (extract(epoch FROM a.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM a.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM assignments a
           JOIN users u ON u.org_id = a.org_id AND u.id = a.assignee_id
           LEFT JOIN courses c
             ON a.target_type = 'course' AND c.org_id = a.org_id AND c.id = a.target_id
           LEFT JOIN modules m
             ON a.target_type = 'module' AND m.org_id = a.org_id AND m.id = a.target_id
           LEFT JOIN module_items mi
             ON a.target_type = 'item' AND mi.org_id = a.org_id AND mi.id = a.target_id
           WHERE a.org_id = $1 AND a.assignee_type = 'user'
             AND ($2::uuid IS NULL OR a.id = $2)
             AND ($3::uuid IS NULL OR a.course_id = $3)
             AND ($4 OR a.ended_at IS NULL)
             AND ($5::uuid IS NULL OR a.id < $5)
           ORDER BY a.id DESC
           LIMIT $6"#,
        scope.org_id(),
        one,
        page.map(|p| p.course_id),
        page.is_none_or(|p| p.include_ended),
        page.and_then(|p| p.after),
        page.map_or(1, |p| p.limit),
    )
    .fetch_all(conn)
    .await?;
    Ok(rows)
}

/// `rows` as assignments, each with its rows' statuses at `now` (B7).
async fn with_counts(
    conn: &mut PgConnection,
    scope: OrgScope,
    rows: Vec<Row>,
    now: SystemTime,
) -> Result<Vec<Assignment>, Error> {
    let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let states = row_states(conn, scope, &ids).await?;
    rows.into_iter()
        .map(|row| {
            let mut counts = Counts::default();
            for (_, state) in states.iter().filter(|(id, _)| *id == row.id) {
                counts.add(state.status(now));
            }
            assignment(row, counts)
        })
        .collect()
}

/// What B7 reads of every row, not removed, of the assignments `ids`: its times and whether the
/// person has any progress on the target (spec 05 B3), by assignment id.
async fn row_states(
    conn: &mut PgConnection,
    scope: OrgScope,
    ids: &[Uuid],
) -> Result<Vec<(Uuid, RowState)>, Error> {
    let rows = sqlx::query!(
        r#"SELECT au.assignment_id,
                  (extract(epoch FROM au.due_at) * 1000000)::bigint AS due_at_us,
                  (extract(epoch FROM au.completed_at) * 1000000)::bigint AS completed_at_us,
                  EXISTS (
                      SELECT 1 FROM item_progress ip
                      LEFT JOIN module_items mi ON mi.org_id = ip.org_id AND mi.id = ip.item_id
                      WHERE ip.org_id = au.org_id AND ip.user_id = au.user_id
                        AND ip.course_id = a.course_id
                        AND (a.target_type = 'course'
                             OR (a.target_type = 'item' AND ip.item_id = a.target_id)
                             OR (a.target_type = 'module' AND mi.module_id = a.target_id))
                  ) AS "has_progress!"
           FROM assignment_users au
           JOIN assignments a ON a.org_id = au.org_id AND a.id = au.assignment_id
           WHERE au.org_id = $1 AND au.assignment_id = ANY($2) AND au.removed_at IS NULL"#,
        scope.org_id(),
        ids,
    )
    .fetch_all(conn)
    .await?;
    rows.into_iter()
        .map(|row| {
            let state = RowState {
                due_at: row.due_at_us.map(from_epoch_micros).transpose()?,
                completed_at: row.completed_at_us.map(from_epoch_micros).transpose()?,
                has_progress: row.has_progress,
            };
            Ok((row.assignment_id, state))
        })
        .collect()
}

fn assignment(row: Row, counts: Counts) -> Result<Assignment, Error> {
    let target_type = TargetType::parse(&row.target_type).ok_or(Error::UnknownStoredValue {
        column: "target_type",
        value: row.target_type.clone(),
    })?;
    let due = match (row.due_fixed_us, row.due_relative) {
        (Some(at), _) => Some(DueRule::Fixed(from_epoch_micros(at)?)),
        (None, Some(days)) => Some(DueRule::RelativeDays(u16::try_from(days).map_err(
            |_| Error::UnknownStoredValue {
                column: "due_relative",
                value: days.to_string(),
            },
        )?)),
        (None, None) => None,
    };
    Ok(Assignment {
        id: row.id,
        course_id: row.course_id,
        target_type,
        target_id: row.target_id,
        target_title: row.target_title,
        user_id: row.user_id,
        user_display_name: row.display_name,
        due,
        created_by: row.created_by,
        ended_at: row.ended_at_us.map(from_epoch_micros).transpose()?,
        created_at: from_epoch_micros(row.created_at_us)?,
        updated_at: from_epoch_micros(row.updated_at_us)?,
        counts,
    })
}
