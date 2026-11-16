// SPDX-License-Identifier: AGPL-3.0-or-later
//! The modules repository (docs/specs/m1/03-modules-and-items.md B1 to B3, B10, B12).
//!
//! A module belongs to one course and holds an ordered list of items ([`crate::items`]). Its
//! `position` is 0-based and dense within the course. Every write here and in
//! [`crate::items`] first locks the course row `FOR NO KEY UPDATE` ([`lock_course`]), so writes
//! to one course's outline run one at a time and each reads the positions the previous one left.
//! The same lock refuses every write to an archived course (spec 02 B5).
//!
//! Who may write is the routes' to decide; this module checks the outline's own rules: titles,
//! the 200-module limit (Q4), deletion only when empty (Q3) and full-set reordering (B10). Every
//! query filters by `org_id = scope.org_id()`.

use std::time::SystemTime;

use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    id::new_id,
    name,
};

/// The most modules a course holds (Q4).
pub const MAX_PER_COURSE: i64 = 200;

/// The longest module or item title, in characters once trimmed: the name rule's limit.
pub const MAX_TITLE_CHARS: usize = name::MAX_CHARS;

/// A module as the repository reads it, without its items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub id: Uuid,
    pub course_id: Uuid,
    pub title: String,
    pub position: i32,
    pub published: bool,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

/// A change to a module; `None` leaves a member as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModulePatch {
    pub title: Option<String>,
    pub published: Option<bool>,
}

/// Why a modules or items repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No course with that id in the scope's organization.
    #[error("the course does not exist")]
    CourseNotFound,
    /// No module with that id in the scope's organization, or, for a move's destination, in the
    /// item's course.
    #[error("the module does not exist")]
    ModuleNotFound,
    /// No item with that id in the scope's organization.
    #[error("the item does not exist")]
    ItemNotFound,
    /// The course is archived and takes no content write (spec 02 B5).
    #[error("the course is archived")]
    CourseArchived,
    /// The course holds [`MAX_PER_COURSE`] modules.
    #[error("the course holds the most modules it may")]
    ModuleLimitReached,
    /// The module holds [`crate::items::MAX_PER_MODULE`] items.
    #[error("the module holds the most items it may")]
    ItemLimitReached,
    /// The module still has items (Q3).
    #[error("the module has items")]
    ModuleNotEmpty,
    /// A reorder's list is not exactly the current set of ids (B10).
    #[error("the list is not the current set")]
    OrderStale,
    #[error("a title cannot be empty")]
    TitleEmpty,
    #[error("a title is longer than {MAX_TITLE_CHARS} characters")]
    TitleTooLong,
    /// A string the caller passed holds NUL, which PostgreSQL `text` cannot store.
    #[error("{field} cannot contain the NUL character")]
    ContainsNul { field: &'static str },
    /// The kind cannot be created by this release (B5).
    #[error("items of this kind cannot be created yet")]
    KindUnavailable,
    /// An external link without a URL.
    #[error("an external link needs a URL")]
    UrlRequired,
    /// A URL longer than [`crate::items::MAX_URL_CHARS`] characters (B7).
    #[error("the URL is too long")]
    UrlInvalid,
    /// A URL on a kind that has none (B5, B8).
    #[error("this kind of item has no URL")]
    UrlUnavailable,
    /// A `ref_id` on a kind that has none (B5, B8).
    #[error("this kind of item has no reference")]
    RefIdUnavailable,
    /// A position outside 0 to the item count of the destination (B4, B11).
    #[error("the position is outside the module")]
    PositionOutOfRange,
    /// A stored value this release cannot read: a kind the table's check keeps from happening.
    #[error("the {column} of {id} cannot be read")]
    Stored { id: Uuid, column: &'static str },
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

struct Row {
    id: Uuid,
    course_id: Uuid,
    title: String,
    position: i32,
    published: bool,
    created_at_us: i64,
    updated_at_us: i64,
}

impl TryFrom<Row> for Module {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        Ok(Self {
            created_at: from_epoch_micros(row.created_at_us)?,
            updated_at: from_epoch_micros(row.updated_at_us)?,
            id: row.id,
            course_id: row.course_id,
            title: row.title,
            position: row.position,
            published: row.published,
        })
    }
}

/// Lock the course `id` of the scope's organization `FOR NO KEY UPDATE` for a content write in
/// the transaction `conn` belongs to (B12): [`Error::CourseNotFound`] when it has no such
/// course, [`Error::CourseArchived`] when the course is archived.
pub(crate) async fn lock_course(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
) -> Result<(), Error> {
    let status = sqlx::query_scalar!(
        "SELECT status FROM courses WHERE org_id = $1 AND id = $2 FOR NO KEY UPDATE",
        scope.org_id(),
        id,
    )
    .fetch_optional(conn)
    .await?
    .ok_or(Error::CourseNotFound)?;
    if status == "archived" {
        return Err(Error::CourseArchived);
    }
    Ok(())
}

/// The module `id` of the scope's organization, or `None`.
pub async fn get(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Module>, Error> {
    sqlx::query_as!(
        Row,
        r#"SELECT id, course_id, title, position, published,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM modules WHERE org_id = $1 AND id = $2"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?
    .map(Module::try_from)
    .transpose()
}

/// The course the module `id` of the scope's organization belongs to, or `None`: what
/// `ModuleTarget` checks grants at.
pub async fn course_of(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Uuid>, Error> {
    Ok(sqlx::query_scalar!(
        "SELECT course_id FROM modules WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?)
}

/// The modules of the course `course_id` of the scope's organization, in order.
pub async fn list_for_course(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    course_id: Uuid,
) -> Result<Vec<Module>, Error> {
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT id, course_id, title, position, published,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM modules WHERE org_id = $1 AND course_id = $2
           ORDER BY position"#,
        scope.org_id(),
        course_id,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(Module::try_from).collect()
}

/// Create an unpublished module titled `title` last in the course `course_id` of the scope's
/// organization, with `created_at` and `updated_at` set to `now` (B1).
///
/// Runs in one transaction (a savepoint when `db` is already one) under the course lock.
pub async fn create<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    course_id: Uuid,
    title: &str,
    now: SystemTime,
) -> Result<Module, Error> {
    let title = valid_title(title)?;
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    lock_course(&mut tx, scope, course_id).await?;
    let count = sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM modules WHERE org_id = $1 AND course_id = $2"#,
        scope.org_id(),
        course_id,
    )
    .fetch_one(&mut *tx)
    .await?;
    if count >= MAX_PER_COURSE {
        return Err(Error::ModuleLimitReached);
    }
    let id = new_id();
    sqlx::query!(
        "INSERT INTO modules (id, org_id, course_id, position, title, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5,
                 timestamptz 'epoch' + $6::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $6::bigint * interval '1 microsecond')",
        id,
        scope.org_id(),
        course_id,
        position(count)?,
        title,
        now,
    )
    .execute(&mut *tx)
    .await?;
    let module = get(&mut *tx, scope, id)
        .await?
        .ok_or(Error::ModuleNotFound)?;
    tx.commit().await?;
    Ok(module)
}

/// Apply `patch` to the module `id` of the scope's organization and set `updated_at` to `now`.
/// A patch that changes nothing writes nothing and returns the module as it is.
///
/// Runs in one transaction (a savepoint when `db` is already one) under the course lock.
pub async fn update<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    patch: &ModulePatch,
    now: SystemTime,
) -> Result<Module, Error> {
    let title = patch.title.as_deref().map(valid_title).transpose()?;
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let module = locked(&mut tx, scope, id).await?;
    let title = title.unwrap_or(&module.title);
    let published = patch.published.unwrap_or(module.published);
    if title == module.title && published == module.published {
        return Ok(module);
    }
    sqlx::query!(
        "UPDATE modules
         SET title = $3, published = $4,
             updated_at = timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
        title,
        published,
        now,
    )
    .execute(&mut *tx)
    .await?;
    let module = get(&mut *tx, scope, id)
        .await?
        .ok_or(Error::ModuleNotFound)?;
    tx.commit().await?;
    Ok(module)
}

/// Delete the module `id` of the scope's organization, which must have no items (B3, Q3), and
/// close the gap in its course's positions.
///
/// Runs in one transaction (a savepoint when `db` is already one) under the course lock.
pub async fn delete<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
) -> Result<(), Error> {
    let mut tx = db.begin().await?;
    let module = locked(&mut tx, scope, id).await?;
    let has_items = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM module_items WHERE org_id = $1 AND module_id = $2)
               AS "exists!""#,
        scope.org_id(),
        id,
    )
    .fetch_one(&mut *tx)
    .await?;
    if has_items {
        return Err(Error::ModuleNotEmpty);
    }
    sqlx::query!(
        "DELETE FROM modules WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "UPDATE modules SET position = position - 1
         WHERE org_id = $1 AND course_id = $2 AND position > $3",
        scope.org_id(),
        module.course_id,
        module.position,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Put the modules of the course `course_id` of the scope's organization in the order of `ids`,
/// which must be exactly the course's current module ids (B10): a missing, extra or repeated id
/// is [`Error::OrderStale`] and changes nothing. The current order writes nothing. Returns the
/// modules in their new order.
///
/// Runs in one transaction (a savepoint when `db` is already one) under the course lock.
pub async fn reorder<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    course_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<Module>, Error> {
    let mut tx = db.begin().await?;
    lock_course(&mut tx, scope, course_id).await?;
    let current = sqlx::query_scalar!(
        "SELECT id FROM modules WHERE org_id = $1 AND course_id = $2 ORDER BY position",
        scope.org_id(),
        course_id,
    )
    .fetch_all(&mut *tx)
    .await?;
    if !same_set(&current, ids) {
        return Err(Error::OrderStale);
    }
    if current != ids {
        sqlx::query!(
            "UPDATE modules m SET position = (u.ord - 1)::integer
             FROM unnest($3::uuid[]) WITH ORDINALITY AS u(id, ord)
             WHERE m.org_id = $1 AND m.course_id = $2 AND m.id = u.id",
            scope.org_id(),
            course_id,
            ids,
        )
        .execute(&mut *tx)
        .await?;
    }
    let modules = list_for_course(&mut *tx, scope, course_id).await?;
    tx.commit().await?;
    Ok(modules)
}

/// The module `id`, read after locking its course in the transaction `conn` belongs to.
async fn locked(conn: &mut PgConnection, scope: OrgScope, id: Uuid) -> Result<Module, Error> {
    let course_id = course_of(&mut *conn, scope, id)
        .await?
        .ok_or(Error::ModuleNotFound)?;
    lock_course(conn, scope, course_id).await?;
    get(&mut *conn, scope, id)
        .await?
        .ok_or(Error::ModuleNotFound)
}

/// Whether `list` holds each id of `current` exactly once and nothing else.
pub(crate) fn same_set(current: &[Uuid], list: &[Uuid]) -> bool {
    let mut current = current.to_vec();
    let mut list = list.to_vec();
    current.sort_unstable();
    list.sort_unstable();
    current == list
}

/// A count of rows as the position after them. Counts are bounded by the 200-row limits.
pub(crate) fn position(count: i64) -> Result<i32, Error> {
    i32::try_from(count).map_err(|_| Error::PositionOutOfRange)
}

pub(crate) fn valid_title(title: &str) -> Result<&str, Error> {
    name::valid(title).map_err(|invalid| match invalid {
        name::Invalid::Empty => Error::TitleEmpty,
        name::Invalid::TooLong => Error::TitleTooLong,
        name::Invalid::ContainsNul => Error::ContainsNul { field: "title" },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reorder_list_must_be_the_current_set_exactly() {
        let (a, b, c) = (new_id(), new_id(), new_id());
        assert!(same_set(&[a, b, c], &[c, a, b]));
        assert!(same_set(&[], &[]));
        assert!(!same_set(&[a, b, c], &[a, b]), "missing");
        assert!(!same_set(&[a, b], &[a, b, c]), "extra");
        assert!(!same_set(&[a, b, c], &[a, b, b]), "repeated");
        assert!(!same_set(&[a, b], &[a, a, b]), "repeated and longer");
    }

    #[test]
    fn titles_are_trimmed_and_bounded() {
        assert_eq!(valid_title("  Week 1 ").unwrap(), "Week 1");
        assert!(matches!(valid_title(" "), Err(Error::TitleEmpty)));
        let over = "a".repeat(MAX_TITLE_CHARS + 1);
        assert!(matches!(valid_title(&over), Err(Error::TitleTooLong)));
        assert!(matches!(
            valid_title("a\0b"),
            Err(Error::ContainsNul { field: "title" })
        ));
    }
}
