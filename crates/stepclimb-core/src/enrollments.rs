// SPDX-License-Identifier: AGPL-3.0-or-later
//! The enrollments repository (docs/specs/m1/02-courses-and-enrollments.md B15 to B20, B27).
//!
//! An enrollment puts one user in one course with one role. An `active` enrollment is the
//! course-scope grant (Q1): `grants::Grants::load` reads it beside the user's role assignments.
//! A `concluded` one grants nothing and is kept as history (Q8). Every query filters by
//! `org_id = scope.org_id()`; the table keeps the course and the user in the row's organization
//! (composite foreign keys) and the role built-in or the organization's own (the
//! `enrollments_role_in_org` trigger).
//!
//! Every write locks the course row `FOR NO KEY UPDATE` first (B19), so enrollment writes in one
//! course take turns with each other and with `courses::set_status`, whose published-to-draft
//! check counts the course's learners ([`has_learners`]). [`create`] reads the role
//! `FOR KEY SHARE`, which a change of the role's permissions waits for.
//!
//! Who may enroll whom with which role (B17's permission and escalation checks, the roles that
//! cannot be enrolled) is the routes' to decide, as is refusing changes to an enrollment an
//! outside source owns. This module checks the rules of the course itself: an archived course
//! takes no new enrollment, and a template takes no enrollment whose role holds
//! `course.participate` (B17, B5, Q5).

use std::time::SystemTime;

use serde_json::{Value, json};
use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope, Source,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    id::new_id,
    perm::{CourseParticipate, Permission},
    roles,
};

/// The composite foreign key that keeps the user in the row's organization.
const USER_FKEY: &str = "enrollments_org_id_user_id_fkey";
/// The foreign key to `roles`.
const ROLE_FKEY: &str = "enrollments_role_id_fkey";
/// The constraint trigger that refuses another organization's custom role.
const ROLE_IN_ORG_TRIGGER: &str = "enrollments_role_in_org";
/// The unique constraint on `(course_id, user_id, role_id)`.
const UNIQUE_CONSTRAINT: &str = "enrollments_course_id_user_id_role_id_key";

/// An enrollment's status (B16, Q8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    /// Grants the role's permissions at the course.
    Active,
    /// Grants nothing; kept as history.
    Concluded,
}

impl Status {
    /// The stored and API name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Concluded => "concluded",
        }
    }

    /// The status stored as `value`, or `None` for a value the table's check refuses.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "concluded" => Some(Self::Concluded),
            _ => None,
        }
    }
}

/// An enrollment as the repository reads it, with its user's display name (B20).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Enrollment {
    pub id: Uuid,
    pub course_id: Uuid,
    pub user_id: Uuid,
    pub user_display_name: String,
    pub role_id: Uuid,
    pub status: Status,
    pub source: Source,
    pub external_id: Option<String>,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

/// An enrollment to create, `active`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewEnrollment {
    pub user_id: Uuid,
    /// A built-in role or a custom role of the scope's organization.
    pub role_id: Uuid,
    /// `Local` for an admin's enrollment; M4's enrollment rules and sync write the others.
    pub source: Source,
    pub external_id: Option<String>,
}

/// The filters of [`list_for_course`]; `None` filters nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListFilter {
    pub status: Option<Status>,
    pub role_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
}

/// The sort key of an enrollment in [`list_for_course`]: the user's display name, then the
/// enrollment's id.
pub type ListKey = (String, Uuid);

/// Why an enrollments repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No enrollment with that id in the scope's organization.
    #[error("the enrollment does not exist")]
    NotFound,
    /// No course with that id in the scope's organization.
    #[error("the course does not exist")]
    CourseNotFound,
    /// No user with that id in the scope's organization.
    #[error("the user does not exist")]
    UserNotFound,
    /// The role is neither built-in nor a custom role of the scope's organization.
    #[error("the role does not exist")]
    RoleNotFound,
    /// The user already holds the role in the course.
    #[error("the user already holds this role in this course")]
    Exists,
    /// The course is archived and takes no new enrollment (B5, B17).
    #[error("an archived course takes no new enrollment")]
    CourseArchived,
    /// The course is a template and the role holds `course.participate` (B17, Q5).
    #[error("a template takes no learner")]
    Template,
    /// A string the caller passed holds NUL, which PostgreSQL `text` cannot store.
    #[error("{field} cannot contain the NUL character")]
    ContainsNul { field: &'static str },
    /// A stored value this release does not know.
    #[error("the stored {column} {value:?} is not one this release knows")]
    UnknownStoredValue { column: &'static str, value: String },
    /// Reading the role failed.
    #[error(transparent)]
    Role(#[from] roles::Error),
    /// A time that cannot be stored or read.
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(sqlx::Error),
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        let constraint = error.as_database_error().and_then(|e| e.constraint());
        match constraint {
            Some(USER_FKEY) => Self::UserNotFound,
            Some(ROLE_FKEY | ROLE_IN_ORG_TRIGGER) => Self::RoleNotFound,
            Some(UNIQUE_CONSTRAINT) => Self::Exists,
            _ => Self::Database(error),
        }
    }
}

/// An `enrollments` row joined to its user, as the queries select it.
struct Row {
    id: Uuid,
    course_id: Uuid,
    user_id: Uuid,
    display_name: String,
    role_id: Uuid,
    status: String,
    source: String,
    external_id: Option<String>,
    created_at_us: i64,
    updated_at_us: i64,
}

impl TryFrom<Row> for Enrollment {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        let status = Status::parse(&row.status).ok_or(Error::UnknownStoredValue {
            column: "status",
            value: row.status,
        })?;
        let source = Source::parse(&row.source).ok_or(Error::UnknownStoredValue {
            column: "source",
            value: row.source,
        })?;
        Ok(Self {
            id: row.id,
            course_id: row.course_id,
            user_id: row.user_id,
            user_display_name: row.display_name,
            role_id: row.role_id,
            status,
            source,
            external_id: row.external_id,
            created_at: from_epoch_micros(row.created_at_us)?,
            updated_at: from_epoch_micros(row.updated_at_us)?,
        })
    }
}

/// The enrollment `id` of the scope's organization, or `None` when it has no such enrollment.
pub async fn get(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Enrollment>, Error> {
    sqlx::query_as!(
        Row,
        r#"SELECT e.id, e.course_id, e.user_id, u.display_name, e.role_id, e.status, e.source,
                  e.external_id,
                  (extract(epoch FROM e.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM e.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM enrollments e JOIN users u ON u.org_id = e.org_id AND u.id = e.user_id
           WHERE e.org_id = $1 AND e.id = $2"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?
    .map(Enrollment::try_from)
    .transpose()
}

/// The course of the enrollment `id` of the scope's organization, or `None` when it has no such
/// enrollment: what `EnrollmentTarget` resolves (API).
pub async fn course_of(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Uuid>, Error> {
    let course = sqlx::query_scalar!(
        "SELECT course_id FROM enrollments WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?;
    Ok(course)
}

/// The enrollments of the course `course_id` of the scope's organization matching `filter`,
/// sorted by the user's display name, then the enrollment's id, starting after `after` (the sort
/// key of the previous page's last enrollment) and at most `limit` of them (B20).
///
/// `after` comes from a client's cursor: it only bounds a query already filtered by the scope.
/// A NUL in its display name is refused with [`Error::ContainsNul`].
pub async fn list_for_course(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    course_id: Uuid,
    filter: ListFilter,
    after: Option<&ListKey>,
    limit: i64,
) -> Result<Vec<Enrollment>, Error> {
    let after_name = after.map(|(name, _)| no_nul("cursor", name)).transpose()?;
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT e.id, e.course_id, e.user_id, u.display_name, e.role_id, e.status, e.source,
                  e.external_id,
                  (extract(epoch FROM e.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM e.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM enrollments e JOIN users u ON u.org_id = e.org_id AND u.id = e.user_id
           WHERE e.org_id = $1 AND e.course_id = $2
             AND ($3::text IS NULL OR e.status = $3)
             AND ($4::uuid IS NULL OR e.role_id = $4)
             AND ($5::uuid IS NULL OR e.user_id = $5)
             AND ($6::text IS NULL OR (u.display_name, e.id) > ($6, $7::uuid))
           ORDER BY u.display_name, e.id
           LIMIT $8"#,
        scope.org_id(),
        course_id,
        filter.status.map(Status::as_str),
        filter.role_id,
        filter.user_id,
        after_name,
        after.map(|(_, id)| *id),
        limit,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(Enrollment::try_from).collect()
}

/// Enroll `new.user_id` in the course `course_id` of the scope's organization with
/// `new.role_id`, `active`, with `created_at` and `updated_at` set to `now`, and write the
/// `enrollment.create` audit entry by `actor` (B17, B27).
///
/// Locks the course row `FOR NO KEY UPDATE` and reads the role `FOR KEY SHARE` (B19). An
/// archived course is [`Error::CourseArchived`]; a template is [`Error::Template`] when the role
/// holds `course.participate`. The table refuses a user or role outside the organization and a
/// repeat of the same course, user and role. Runs in one transaction (a savepoint when `db` is
/// already one).
pub async fn create<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    course_id: Uuid,
    new: &NewEnrollment,
    now: SystemTime,
    actor: Actor,
) -> Result<Enrollment, Error> {
    let external_id = new
        .external_id
        .as_deref()
        .map(|id| no_nul("external_id", id))
        .transpose()?;
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let course = lock_course(&mut tx, scope, course_id).await?;
    let role = roles::get_for_key_share(&mut tx, scope, new.role_id)
        .await?
        .ok_or(Error::RoleNotFound)?;
    if course.status == "archived" {
        return Err(Error::CourseArchived);
    }
    let participates = role.permissions.iter().any(|k| k == CourseParticipate::KEY);
    if course.template && participates {
        return Err(Error::Template);
    }
    let id = new_id();
    sqlx::query!(
        "INSERT INTO enrollments (id, org_id, course_id, user_id, role_id, source, external_id,
                                  created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7,
                 timestamptz 'epoch' + $8::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $8::bigint * interval '1 microsecond')",
        id,
        scope.org_id(),
        course_id,
        new.user_id,
        new.role_id,
        new.source.as_str(),
        external_id,
        now,
    )
    .execute(&mut *tx)
    .await?;
    let created = get(&mut *tx, scope, id).await?.ok_or(Error::NotFound)?;
    let data = json!({ "after": audit_data(&created) });
    audit::record(
        &mut tx,
        scope,
        actor,
        Action::EnrollmentCreate,
        id,
        &data,
        now,
    )
    .await?;
    tx.commit().await?;
    Ok(created)
}

/// Change the status of the enrollment `id` of the scope's organization to `to`, set
/// `updated_at` to `now`, and write the `enrollment.update` audit entry by `actor` with the
/// status before and after (B18, B27). The current status writes nothing and returns the
/// enrollment as it is.
///
/// Locks the enrollment's course row `FOR NO KEY UPDATE` first (B19). Runs in one transaction
/// (a savepoint when `db` is already one).
pub async fn set_status<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    to: Status,
    now: SystemTime,
    actor: Actor,
) -> Result<Enrollment, Error> {
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let current = lock(&mut tx, scope, id).await?;
    if current.status == to {
        return Ok(current);
    }
    sqlx::query!(
        "UPDATE enrollments
         SET status = $3, updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
        to.as_str(),
        now,
    )
    .execute(&mut *tx)
    .await?;
    let data = json!({
        "before": { "status": current.status.as_str() },
        "after": { "status": to.as_str() },
    });
    audit::record(
        &mut tx,
        scope,
        actor,
        Action::EnrollmentUpdate,
        id,
        &data,
        now,
    )
    .await?;
    let updated = get(&mut *tx, scope, id).await?.ok_or(Error::NotFound)?;
    tx.commit().await?;
    Ok(updated)
}

/// Delete the enrollment `id` of the scope's organization and write the `enrollment.delete`
/// audit entry by `actor` at `now`, with the enrollment as it was (B18, B27).
///
/// Locks the enrollment's course row `FOR NO KEY UPDATE` first (B19). Runs in one transaction
/// (a savepoint when `db` is already one).
pub async fn delete<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    now: SystemTime,
    actor: Actor,
) -> Result<(), Error> {
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let current = lock(&mut tx, scope, id).await?;
    sqlx::query!(
        "DELETE FROM enrollments WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
    )
    .execute(&mut *tx)
    .await?;
    let data = json!({ "before": audit_data(&current) });
    audit::record(
        &mut tx,
        scope,
        actor,
        Action::EnrollmentDelete,
        id,
        &data,
        now,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// The enrollment `id` after locking its course row `FOR NO KEY UPDATE` (B19), or
/// [`Error::NotFound`]. An enrollment never changes course, so the course read first is still
/// its course once locked.
async fn lock(conn: &mut PgConnection, scope: OrgScope, id: Uuid) -> Result<Enrollment, Error> {
    let course_id = course_of(&mut *conn, scope, id)
        .await?
        .ok_or(Error::NotFound)?;
    lock_course(conn, scope, course_id)
        .await
        .map_err(|error| match error {
            Error::CourseNotFound => Error::NotFound,
            other => other,
        })?;
    get(&mut *conn, scope, id).await?.ok_or(Error::NotFound)
}

/// What [`create`] and [`delete`] read of a course while holding its row.
struct LockedCourse {
    status: String,
    template: bool,
}

/// Lock the course `id` of the scope's organization `FOR NO KEY UPDATE` until the transaction
/// ends, or [`Error::CourseNotFound`].
async fn lock_course(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
) -> Result<LockedCourse, Error> {
    sqlx::query_as!(
        LockedCourse,
        "SELECT status, template FROM courses WHERE org_id = $1 AND id = $2 FOR NO KEY UPDATE",
        scope.org_id(),
        id,
    )
    .fetch_optional(conn)
    .await?
    .ok_or(Error::CourseNotFound)
}

/// Whether the course `course_id` of the scope's organization has an `active` enrollment whose
/// role holds `course.participate`: a learner, which keeps a published course from going back
/// to draft (B4). For `courses::set_status`, in its transaction after it locked the course row,
/// which every enrollment write locks first. The custom roles read are locked `FOR KEY SHARE`,
/// so a change of their permissions waits until the caller commits.
pub(crate) async fn has_learners(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let roles = sqlx::query!(
        "SELECT r.key, r.builtin, r.permissions
         FROM roles r
         WHERE (r.org_id IS NULL OR r.org_id = $1)
           AND r.id IN (SELECT e.role_id FROM enrollments e
                        WHERE e.org_id = $1 AND e.course_id = $2 AND e.status = 'active')
         ORDER BY r.id
         FOR KEY SHARE",
        scope.org_id(),
        course_id,
    )
    .fetch_all(conn)
    .await?;
    Ok(roles.into_iter().any(|role| {
        roles::effective_permissions(&role.key, role.builtin, role.permissions)
            .iter()
            .any(|key| key == CourseParticipate::KEY)
    }))
}

/// What an audit entry records of an enrollment: who holds which role in which course, its
/// status and its source.
fn audit_data(enrollment: &Enrollment) -> Value {
    json!({
        "course_id": enrollment.course_id,
        "user_id": enrollment.user_id,
        "role_id": enrollment.role_id,
        "status": enrollment.status.as_str(),
        "source": enrollment.source.as_str(),
    })
}

/// `value` unchanged, or [`Error::ContainsNul`] naming `field`.
fn no_nul<'a>(field: &'static str, value: &'a str) -> Result<&'a str, Error> {
    if value.contains('\0') {
        return Err(Error::ContainsNul { field });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_round_trip_their_names() {
        for status in [Status::Active, Status::Concluded] {
            assert_eq!(Status::parse(status.as_str()), Some(status));
        }
        assert_eq!(Status::parse("Active"), None);
        assert_eq!(Status::parse("deleted"), None);
    }
}
