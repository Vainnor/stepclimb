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
//!
//! An enrollment whose role holds `progress.view_observed` (an Observer) names the learners it
//! observes in `enrollment_observees` (spec 05 B17, Q4): 1 to [`MAX_OBSERVED`] users, each
//! holding an enrollment in the course, active or concluded, whose role has
//! `course.participate`. Any other enrollment names none. Who may name them (spec 05 B19) is the
//! routes' to decide.
//!
//! [`delete`] removes, in its transaction, the enrollment's own observer links and, when the
//! user keeps no learner enrollment in the course, the links that name them (spec 05 B15, B20).
//! It refuses to delete the last learner enrollment of a user with progress in the course, so
//! no learner history is erased before spec 14 I4 archives it.

use std::{collections::HashSet, time::SystemTime};

use serde_json::{Value, json};
use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope, Source,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    id::new_id,
    perm::{CourseParticipate, Permission, ProgressViewObserved},
    roles,
};

/// The most learners one Observer enrollment observes (spec 05 B17).
pub const MAX_OBSERVED: usize = 100;

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
    /// The learners the enrollment observes, sorted by id: `Some` when its role holds
    /// `progress.view_observed` (spec 05 B17), `None` otherwise.
    pub observed_user_ids: Option<Vec<Uuid>>,
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
    /// The learners to observe: 1 to [`MAX_OBSERVED`] when the role holds
    /// `progress.view_observed`, empty otherwise (spec 05 B17).
    pub observed_user_ids: Vec<Uuid>,
}

/// A change to an enrollment (B18; spec 05 B17); `None` keeps the current value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Change {
    pub status: Option<Status>,
    /// The learners the enrollment observes, replacing the current list whole.
    pub observed_user_ids: Option<Vec<Uuid>>,
}

/// The rows [`delete`] removed besides the enrollment (spec 05 B15).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Removed {
    pub enrollment_observees: u64,
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
    /// Observed learners were named for an enrollment whose role does not hold
    /// `progress.view_observed` (spec 05 B17).
    #[error("only an enrollment whose role holds progress.view_observed observes learners")]
    ObservedUnavailable,
    /// The user keeps no other learner enrollment in the course and has progress there, which a
    /// delete would erase (spec 05 B15, until spec 14 I4 archives it first).
    #[error("the learner has progress in the course; conclude the enrollment instead")]
    HasProgress,
    /// The observed learners are not 1 to [`MAX_OBSERVED`] distinct learners of the course:
    /// `index` names the first entry at fault, or is `None` when the list's length is.
    #[error("the observed learners are invalid")]
    ObservedInvalid { index: Option<usize> },
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
    role_key: String,
    role_builtin: bool,
    role_permissions: Vec<String>,
    observed: Vec<Uuid>,
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
        let observes =
            roles::effective_permissions(&row.role_key, row.role_builtin, row.role_permissions)
                .iter()
                .any(|key| key == ProgressViewObserved::KEY);
        Ok(Self {
            id: row.id,
            course_id: row.course_id,
            user_id: row.user_id,
            user_display_name: row.display_name,
            role_id: row.role_id,
            status,
            source,
            external_id: row.external_id,
            observed_user_ids: observes.then_some(row.observed),
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
                  e.external_id, r.key AS role_key, r.builtin AS role_builtin,
                  r.permissions AS role_permissions,
                  ARRAY(SELECT o.user_id FROM enrollment_observees o
                        WHERE o.org_id = e.org_id AND o.enrollment_id = e.id
                        ORDER BY o.user_id) AS "observed!",
                  (extract(epoch FROM e.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM e.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM enrollments e JOIN users u ON u.org_id = e.org_id AND u.id = e.user_id
           JOIN roles r ON r.id = e.role_id AND (r.org_id IS NULL OR r.org_id = e.org_id)
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
                  e.external_id, r.key AS role_key, r.builtin AS role_builtin,
                  r.permissions AS role_permissions,
                  ARRAY(SELECT o.user_id FROM enrollment_observees o
                        WHERE o.org_id = e.org_id AND o.enrollment_id = e.id
                        ORDER BY o.user_id) AS "observed!",
                  (extract(epoch FROM e.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM e.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM enrollments e JOIN users u ON u.org_id = e.org_id AND u.id = e.user_id
           JOIN roles r ON r.id = e.role_id AND (r.org_id IS NULL OR r.org_id = e.org_id)
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
/// `new.role_id`, `active`, observing `new.observed_user_ids`, with `created_at` and
/// `updated_at` set to `now`, and write the `enrollment.create` audit entry by `actor` (B17,
/// B27; spec 05 B17).
///
/// Locks the course row `FOR NO KEY UPDATE` and reads the role `FOR KEY SHARE` (B19). An
/// archived course is [`Error::CourseArchived`]; a template is [`Error::Template`] when the role
/// holds `course.participate`. The table refuses a user or role outside the organization and a
/// repeat of the same course, user and role. Observed learners on a role without
/// `progress.view_observed` are [`Error::ObservedUnavailable`]; a list that is not 1 to
/// [`MAX_OBSERVED`] distinct learners of the course on a role with it is
/// [`Error::ObservedInvalid`]. Runs in one transaction (a savepoint when `db` is already one).
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
    let now_us = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let course = lock_course(&mut tx, scope, course_id).await?;
    let role = roles::get_for_key_share(&mut tx, scope, new.role_id)
        .await?
        .ok_or(Error::RoleNotFound)?;
    if course.status == "archived" {
        return Err(Error::CourseArchived);
    }
    if course.template && holds(&role, CourseParticipate::KEY) {
        return Err(Error::Template);
    }
    let observes = holds(&role, ProgressViewObserved::KEY);
    if observes {
        check_observed(&mut tx, scope, course_id, &new.observed_user_ids).await?;
    } else if !new.observed_user_ids.is_empty() {
        return Err(Error::ObservedUnavailable);
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
        now_us,
    )
    .execute(&mut *tx)
    .await?;
    insert_observed(
        &mut tx,
        scope,
        course_id,
        id,
        &new.observed_user_ids,
        now_us,
    )
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
        now_us,
    )
    .await?;
    tx.commit().await?;
    Ok(created)
}

/// Change the status of the enrollment `id` of the scope's organization to `to`: [`update`]
/// with only a status.
pub async fn set_status<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    to: Status,
    now: SystemTime,
    actor: Actor,
) -> Result<Enrollment, Error> {
    let change = Change {
        status: Some(to),
        observed_user_ids: None,
    };
    update(db, scope, id, &change, now, actor).await
}

/// Apply `change` to the enrollment `id` of the scope's organization, set `updated_at` to
/// `now`, and write one `enrollment.update` audit entry by `actor` with the changed members
/// before and after (B18, B27; spec 05 B17). A change naming only current values writes nothing
/// and returns the enrollment as it is.
///
/// Reactivating in a template is [`Error::Template`] when the role holds `course.participate`,
/// as [`create`] refuses it (Q5). A new list of observed learners replaces the current one
/// whole, with [`create`]'s checks. The role is read `FOR KEY SHARE`. Locks the enrollment's
/// course row `FOR NO KEY UPDATE` first (B19). Runs in one transaction (a savepoint when `db`
/// is already one).
pub async fn update<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    change: &Change,
    now: SystemTime,
    actor: Actor,
) -> Result<Enrollment, Error> {
    let now_us = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let (current, course) = lock(&mut tx, scope, id).await?;
    let role = roles::get_for_key_share(&mut tx, scope, current.role_id)
        .await?
        .ok_or(Error::RoleNotFound)?;
    let status = change.status.filter(|to| *to != current.status);
    let observed = match &change.observed_user_ids {
        Some(list) => new_observed(&mut tx, scope, &current, &role, list).await?,
        None => None,
    };
    if status.is_none() && observed.is_none() {
        return Ok(current);
    }
    if status == Some(Status::Active) && course.template && holds(&role, CourseParticipate::KEY) {
        return Err(Error::Template);
    }
    let (mut before, mut after) = (json!({}), json!({}));
    if let Some(to) = status {
        before["status"] = json!(current.status.as_str());
        after["status"] = json!(to.as_str());
    }
    if let Some(list) = &observed {
        delete_own_links(&mut tx, scope, id).await?;
        insert_observed(&mut tx, scope, current.course_id, id, list, now_us).await?;
        before["observed_user_ids"] = json!(current.observed_user_ids);
        after["observed_user_ids"] = json!(list);
    }
    sqlx::query!(
        "UPDATE enrollments
         SET status = coalesce($3, status),
             updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
        status.map(Status::as_str),
        now_us,
    )
    .execute(&mut *tx)
    .await?;
    let data = json!({ "before": before, "after": after });
    audit::record(
        &mut tx,
        scope,
        actor,
        Action::EnrollmentUpdate,
        id,
        &data,
        now_us,
    )
    .await?;
    let updated = get(&mut *tx, scope, id).await?.ok_or(Error::NotFound)?;
    tx.commit().await?;
    Ok(updated)
}

/// Delete the enrollment `id` of the scope's organization and write the `enrollment.delete`
/// audit entry by `actor` at `now`, with the enrollment as it was and the observer links
/// removed (B18, B27; spec 05 B15, B20).
///
/// Deleting an enrollment keeps the learner's history (owner decision, 2026-11-16): spec 05
/// B15 archives live progress first, which spec 14 I4 builds. Until then, when no other
/// enrollment of the user in the course, active or concluded, has a role holding
/// `course.participate`, and the user has any `item_progress`, `module_progress` or
/// `course_progress` row in the course, the delete is [`Error::HasProgress`] and writes nothing:
/// the enrollment is concluded instead. This does not depend on the deleted enrollment's role,
/// so deleting a staff or Observer enrollment of a former learner is refused the same way.
///
/// Otherwise it deletes the enrollment's own observer links and, when the user keeps no
/// learner enrollment in the course, the links that name them there. The user's `completions`
/// in the course are history and stay in every case (spec 05 B15, spec 14 B16). Locks the enrollment's
/// course row `FOR NO KEY UPDATE` first (B19), which every progress write's `FOR SHARE` read
/// waits for, so no progress appears between the check and the delete. Runs in one transaction
/// (a savepoint when `db` is already one).
pub async fn delete<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    now: SystemTime,
    actor: Actor,
) -> Result<Removed, Error> {
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let (current, _) = lock(&mut tx, scope, id).await?;
    let (course_id, user_id) = (current.course_id, current.user_id);
    let stays_learner = participates(&mut tx, scope, course_id, user_id, false, Some(id)).await?;
    if !stays_learner && has_live_progress(&mut tx, scope, course_id, user_id).await? {
        return Err(Error::HasProgress);
    }
    let mut removed = Removed {
        enrollment_observees: delete_own_links(&mut tx, scope, id).await?,
    };
    sqlx::query!(
        "DELETE FROM enrollments WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
    )
    .execute(&mut *tx)
    .await?;
    if !stays_learner {
        removed.enrollment_observees += sqlx::query!(
            "DELETE FROM enrollment_observees
             WHERE org_id = $1 AND course_id = $2 AND user_id = $3",
            scope.org_id(),
            course_id,
            user_id,
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
    }
    let data = json!({
        "before": audit_data(&current),
        "removed": { "enrollment_observees": removed.enrollment_observees },
    });
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
    Ok(removed)
}

/// The sorted list `list` to store as the learners `current` observes, after [`create`]'s
/// checks, or `None` when it is the current list. [`Error::ObservedUnavailable`] when `role`
/// does not hold `progress.view_observed`.
async fn new_observed(
    conn: &mut PgConnection,
    scope: OrgScope,
    current: &Enrollment,
    role: &roles::Role,
    list: &[Uuid],
) -> Result<Option<Vec<Uuid>>, Error> {
    if !holds(role, ProgressViewObserved::KEY) {
        return Err(Error::ObservedUnavailable);
    }
    check_observed(conn, scope, current.course_id, list).await?;
    let mut sorted = list.to_vec();
    sorted.sort_unstable();
    Ok((current.observed_user_ids.as_ref() != Some(&sorted)).then_some(sorted))
}

/// Delete the observer links of the enrollment `id`: how many there were.
async fn delete_own_links(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
) -> Result<u64, Error> {
    let deleted = sqlx::query!(
        "DELETE FROM enrollment_observees WHERE org_id = $1 AND enrollment_id = $2",
        scope.org_id(),
        id,
    )
    .execute(conn)
    .await?;
    Ok(deleted.rows_affected())
}

/// Whether the user `user_id` has any live progress row (`item_progress`, `module_progress` or
/// `course_progress`) in the course `course_id` of the scope's organization.
async fn has_live_progress(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    user_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM item_progress
                          WHERE org_id = $1 AND course_id = $2 AND user_id = $3)
               OR EXISTS (SELECT 1 FROM module_progress
                          WHERE org_id = $1 AND course_id = $2 AND user_id = $3)
               OR EXISTS (SELECT 1 FROM course_progress
                          WHERE org_id = $1 AND course_id = $2 AND user_id = $3) AS "exists!""#,
        scope.org_id(),
        course_id,
        user_id,
    )
    .fetch_one(conn)
    .await
}

/// Whether the enrollment `id` has observer link rows, whatever its role holds now: a stale
/// link of a role that lost `progress.view_observed` would grant sight again if the role got
/// it back (spec 05 B19).
pub async fn has_observer_links(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
) -> Result<bool, Error> {
    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM enrollment_observees
                          WHERE org_id = $1 AND enrollment_id = $2) AS "exists!""#,
        scope.org_id(),
        id,
    )
    .fetch_one(conn)
    .await?;
    Ok(exists)
}

/// Whether `role`'s effective permissions include `key`.
fn holds(role: &roles::Role, key: &str) -> bool {
    role.permissions.iter().any(|held| held == key)
}

/// [`Error::ObservedInvalid`] unless `users` is 1 to [`MAX_OBSERVED`] distinct users, each
/// holding an enrollment in the course `course_id`, active or concluded, whose role has
/// `course.participate` (spec 05 B17). For the writes, after they locked the course row, which
/// every enrollment write locks first; the custom roles read are locked `FOR KEY SHARE`, so a
/// change of their permissions waits until the caller commits.
async fn check_observed(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    users: &[Uuid],
) -> Result<(), Error> {
    if users.is_empty() || users.len() > MAX_OBSERVED {
        return Err(Error::ObservedInvalid { index: None });
    }
    let mut seen = HashSet::new();
    if let Some(index) = users.iter().position(|user| !seen.insert(*user)) {
        return Err(Error::ObservedInvalid { index: Some(index) });
    }
    let rows = sqlx::query!(
        "SELECT e.user_id, r.key, r.builtin, r.permissions
         FROM enrollments e
         JOIN roles r ON r.id = e.role_id AND (r.org_id IS NULL OR r.org_id = e.org_id)
         WHERE e.org_id = $1 AND e.course_id = $2 AND e.user_id = ANY($3)
         ORDER BY r.id
         FOR KEY SHARE OF r",
        scope.org_id(),
        course_id,
        users,
    )
    .fetch_all(&mut *conn)
    .await?;
    let learners: HashSet<Uuid> = rows
        .into_iter()
        .filter(|row| {
            roles::effective_permissions(&row.key, row.builtin, row.permissions.clone())
                .iter()
                .any(|key| key == CourseParticipate::KEY)
        })
        .map(|row| row.user_id)
        .collect();
    match users.iter().position(|user| !learners.contains(user)) {
        Some(index) => Err(Error::ObservedInvalid { index: Some(index) }),
        None => Ok(()),
    }
}

/// Insert one `enrollment_observees` row per user in `users` for the enrollment `id` of the
/// course `course_id`, which [`check_observed`] passed.
async fn insert_observed(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    id: Uuid,
    users: &[Uuid],
    now_us: i64,
) -> Result<(), Error> {
    if users.is_empty() {
        return Ok(());
    }
    sqlx::query!(
        "INSERT INTO enrollment_observees (org_id, enrollment_id, course_id, user_id,
                                           created_at, updated_at)
         SELECT $1, $2, $3, observed,
                timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
         FROM unnest($4::uuid[]) AS observed",
        scope.org_id(),
        id,
        course_id,
        users,
        now_us,
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// The learners named by the `active` enrollments of `observer` in the course `course_id` of
/// the scope's organization whose role holds `progress.view_observed`, sorted by id: whom an
/// Observer reads (spec 05 B20). A concluded enrollment, or one whose role no longer holds the
/// permission, names no one.
pub async fn observed_by(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    observer: Uuid,
) -> Result<Vec<Uuid>, Error> {
    let observer_roles = roles::ids_holding(&mut *conn, scope, ProgressViewObserved::KEY).await?;
    let users = sqlx::query_scalar!(
        r#"SELECT DISTINCT o.user_id AS "user_id!"
           FROM enrollment_observees o
           JOIN enrollments e ON e.org_id = o.org_id AND e.id = o.enrollment_id
           WHERE o.org_id = $1 AND o.course_id = $2 AND e.user_id = $3 AND e.status = 'active'
             AND e.role_id = ANY($4)
           ORDER BY 1"#,
        scope.org_id(),
        course_id,
        observer,
        &observer_roles,
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(users)
}

/// The enrollment `id` and what [`lock_course`] read of its course, after locking the course
/// row `FOR NO KEY UPDATE` (B19), or [`Error::NotFound`]. An enrollment never changes course, so the course read first is still
/// its course once locked.
async fn lock(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
) -> Result<(Enrollment, LockedCourse), Error> {
    let course_id = course_of(&mut *conn, scope, id)
        .await?
        .ok_or(Error::NotFound)?;
    let course = lock_course(conn, scope, course_id)
        .await
        .map_err(|error| match error {
            Error::CourseNotFound => Error::NotFound,
            other => other,
        })?;
    let enrollment = get(&mut *conn, scope, id).await?.ok_or(Error::NotFound)?;
    Ok((enrollment, course))
}

/// What the writes read of a course while holding its row.
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

/// Whether the user `user_id` holds an `active` enrollment in the course `course_id` of the
/// scope's organization whose role holds `course.participate`: a learner (spec 05 B1). For
/// `progress`, in its transaction after it read the course row `FOR SHARE`, which every
/// enrollment write's `FOR NO KEY UPDATE` waits for.
pub(crate) async fn is_learner(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    user_id: Uuid,
) -> Result<bool, sqlx::Error> {
    participates(conn, scope, course_id, user_id, true, None).await
}

/// Whether the user `user_id` holds an enrollment in the course `course_id` of the scope's
/// organization whose role holds `course.participate`: an `active` one when `active_only`,
/// otherwise active or concluded, leaving out the enrollment `except`. The custom roles read are
/// locked `FOR KEY SHARE`, as in [`has_learners`], so a change of their permissions waits until
/// the caller commits.
async fn participates(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    user_id: Uuid,
    active_only: bool,
    except: Option<Uuid>,
) -> Result<bool, sqlx::Error> {
    let roles = sqlx::query!(
        "SELECT r.key, r.builtin, r.permissions
         FROM roles r
         WHERE (r.org_id IS NULL OR r.org_id = $1)
           AND r.id IN (SELECT e.role_id FROM enrollments e
                        WHERE e.org_id = $1 AND e.course_id = $2 AND e.user_id = $3
                          AND (e.status = 'active' OR NOT $4)
                          AND ($5::uuid IS NULL OR e.id <> $5))
         ORDER BY r.id
         FOR KEY SHARE",
        scope.org_id(),
        course_id,
        user_id,
        active_only,
        except,
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
/// status and its source, and for an Observer the learners it observes.
fn audit_data(enrollment: &Enrollment) -> Value {
    let mut data = json!({
        "course_id": enrollment.course_id,
        "user_id": enrollment.user_id,
        "role_id": enrollment.role_id,
        "status": enrollment.status.as_str(),
        "source": enrollment.source.as_str(),
    });
    if let Some(observed) = &enrollment.observed_user_ids {
        data["observed_user_ids"] = json!(observed);
    }
    data
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
