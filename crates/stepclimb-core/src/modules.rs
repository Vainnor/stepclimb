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

use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};
use uuid::Uuid;

pub use crate::rules::CompletionRule;
use crate::{
    OrgScope, completions,
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
    /// The module stays locked before this time (spec 04 B8).
    pub unlock_at: Option<SystemTime>,
    /// The modules it waits for, in course order (spec 04 B2).
    pub prerequisite_ids: Vec<Uuid>,
    /// Whether its items go in order (spec 04 B9).
    pub require_sequential: bool,
    pub completion_rule: CompletionRule,
    /// How many months a completion of the module stays valid, 1 to 120; `None` for ever
    /// (spec 14 B3).
    pub completion_valid_months: Option<i16>,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

/// A change to a module; `None` leaves a member as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModulePatch {
    pub title: Option<String>,
    pub published: Option<bool>,
    /// `Some(None)` clears the validity (spec 14 B3).
    pub completion_valid_months: Option<Option<i16>>,
}

/// A change to a module's rules (spec 04 B1); `None` leaves a member as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RulesPatch {
    /// `Some(None)` clears the date.
    pub unlock_at: Option<Option<SystemTime>>,
    /// The whole new set, which replaces the current one.
    pub prerequisite_ids: Option<Vec<Uuid>>,
    pub require_sequential: Option<bool>,
    pub completion_rule: Option<CompletionRule>,
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
    /// The item has learner progress and cannot be deleted; unpublish it instead (spec 05 B16).
    #[error("the item has learner progress")]
    ItemHasProgress,
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
    /// A URL `items::url_is_valid` refuses (B7).
    #[error("the URL is not an absolute http or https link")]
    UrlInvalid,
    /// A URL on a kind that has none (B5, B8).
    #[error("this kind of item has no URL")]
    UrlUnavailable,
    /// A `ref_id` on a kind that has none (B5, B8).
    #[error("this kind of item has no reference")]
    RefIdUnavailable,
    /// A kind that names content without a `ref_id` (B5; spec 06 B13).
    #[error("this kind of item needs a reference")]
    RefIdRequired,
    /// A `file` item's `ref_id` that is not a course file of the item's course (spec 06 B13).
    #[error("the course file does not exist")]
    FileNotFound,
    /// A position outside 0 to the item count of the destination (B4, B11).
    #[error("the position is outside the module")]
    PositionOutOfRange,
    /// A module named as its own prerequisite (spec 04 B2).
    #[error("a module cannot require itself")]
    PrerequisiteSelf,
    /// The same prerequisite named twice.
    #[error("a prerequisite is named twice")]
    PrerequisiteDuplicate,
    /// More than [`crate::rules::MAX_PREREQUISITES`] prerequisites.
    #[error("a module names too many prerequisites")]
    PrerequisitesTooMany,
    /// A prerequisite that is not a module of the same course: answered as a module that does
    /// not exist.
    #[error("a prerequisite is not a module of the course")]
    PrerequisiteNotFound,
    /// The prerequisites would close a cycle in the course's graph (spec 04 B2, Q2).
    #[error("the prerequisites would form a cycle")]
    PrerequisiteCycle,
    /// A `completion_valid_months` outside 1 to 120 (spec 14 B3).
    #[error("a completion validity must be from 1 to 120 months")]
    ValidityOutOfRange,
    /// An `unlock_at` before 1970 or from 2200 on.
    #[error("an unlock date must be from 1970 up to 2200")]
    UnlockAtOutOfRange,
    /// A requirement of an unknown shape, or on an item kind that does not take it (spec 04 B3).
    #[error("the requirement is not valid for this item")]
    RequirementInvalid,
    /// A requirement kind reserved for M2 or M3 (spec 04 B3).
    #[error("this requirement kind is not available yet")]
    RequirementUnavailable,
    /// The transaction runs at an isolation level other than `READ COMMITTED`, where the course
    /// lock does not protect positions or the prerequisite graph ([`lock_course`]).
    #[error("course content can be changed only at READ COMMITTED isolation, not {found}")]
    IsolationLevel { found: String },
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
    unlock_at_us: Option<i64>,
    prerequisite_ids: Vec<Uuid>,
    require_sequential: bool,
    completion_rule: String,
    completion_valid_months: Option<i16>,
    created_at_us: i64,
    updated_at_us: i64,
}

impl TryFrom<Row> for Module {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        Ok(Self {
            completion_rule: CompletionRule::parse(&row.completion_rule).ok_or(Error::Stored {
                id: row.id,
                column: "completion_rule",
            })?,
            unlock_at: row.unlock_at_us.map(from_epoch_micros).transpose()?,
            created_at: from_epoch_micros(row.created_at_us)?,
            updated_at: from_epoch_micros(row.updated_at_us)?,
            id: row.id,
            course_id: row.course_id,
            title: row.title,
            position: row.position,
            published: row.published,
            prerequisite_ids: row.prerequisite_ids,
            require_sequential: row.require_sequential,
            completion_valid_months: row.completion_valid_months,
        })
    }
}

/// Lock the course `id` of the scope's organization `FOR NO KEY UPDATE` for a content write in
/// the transaction `conn` belongs to (B12): [`Error::CourseNotFound`] when it has no such
/// course, [`Error::CourseArchived`] when the course is archived.
///
/// The lock keeps positions dense and the prerequisite graph acyclic only at `READ COMMITTED`,
/// PostgreSQL's default, where each statement after the lock reads what the previous holder
/// committed. Under `REPEATABLE READ` or `SERIALIZABLE` a write would read the snapshot taken
/// before it waited, so any other level is refused with [`Error::IsolationLevel`].
pub(crate) async fn lock_course(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
) -> Result<(), Error> {
    let locked = sqlx::query!(
        r#"SELECT status, current_setting('transaction_isolation') AS "isolation!"
           FROM courses WHERE org_id = $1 AND id = $2 FOR NO KEY UPDATE"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(conn)
    .await?
    .ok_or(Error::CourseNotFound)?;
    if locked.isolation != "read committed" {
        return Err(Error::IsolationLevel {
            found: locked.isolation,
        });
    }
    if locked.status == "archived" {
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
        r#"SELECT m.id, m.course_id, m.title, m.position, m.published,
                  (extract(epoch FROM m.unlock_at) * 1000000)::bigint AS unlock_at_us,
                  ARRAY(SELECT p.requires_id
                        FROM module_prerequisites p
                        JOIN modules r ON r.org_id = p.org_id AND r.id = p.requires_id
                        WHERE p.org_id = m.org_id AND p.module_id = m.id
                        ORDER BY r.position) AS "prerequisite_ids!",
                  m.require_sequential, m.completion_rule, m.completion_valid_months,
                  (extract(epoch FROM m.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM m.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM modules m WHERE m.org_id = $1 AND m.id = $2"#,
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
        r#"SELECT m.id, m.course_id, m.title, m.position, m.published,
                  (extract(epoch FROM m.unlock_at) * 1000000)::bigint AS unlock_at_us,
                  ARRAY(SELECT p.requires_id
                        FROM module_prerequisites p
                        JOIN modules r ON r.org_id = p.org_id AND r.id = p.requires_id
                        WHERE p.org_id = m.org_id AND p.module_id = m.id
                        ORDER BY r.position) AS "prerequisite_ids!",
                  m.require_sequential, m.completion_rule, m.completion_valid_months,
                  (extract(epoch FROM m.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM m.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM modules m WHERE m.org_id = $1 AND m.course_id = $2
           ORDER BY m.position"#,
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
/// A patch that changes nothing writes nothing and returns the module as it is. A validity
/// outside 1 to 120 months is [`Error::ValidityOutOfRange`]; a new one applies to completions
/// recorded afterwards (spec 14 B3, Q3).
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
    if let Some(Some(months)) = patch.completion_valid_months
        && !completions::valid_months(months)
    {
        return Err(Error::ValidityOutOfRange);
    }
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let module = locked(&mut tx, scope, id).await?;
    let title = title.unwrap_or(&module.title);
    let published = patch.published.unwrap_or(module.published);
    let validity = patch
        .completion_valid_months
        .unwrap_or(module.completion_valid_months);
    if title == module.title
        && published == module.published
        && validity == module.completion_valid_months
    {
        return Ok(module);
    }
    sqlx::query!(
        "UPDATE modules
         SET title = $3, published = $4, completion_valid_months = $6,
             updated_at = timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
        title,
        published,
        now,
        validity,
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

/// Apply `patch` to the rules of the module `id` of the scope's organization and set
/// `updated_at` to `now` (spec 04 B1, B2). A new prerequisite set replaces the old one; it must
/// name modules of the same course, not the module itself, at most
/// [`crate::rules::MAX_PREREQUISITES`] of them, each once, and close no cycle in the course's
/// graph. A patch that changes nothing writes nothing and returns the module as it is.
///
/// Runs in one transaction (a savepoint when `db` is already one) under the course lock, so the
/// cycle check reads every other write's prerequisites and two writes that each close half a
/// cycle cannot both pass.
pub async fn set_rules<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    patch: &RulesPatch,
    now: SystemTime,
) -> Result<Module, Error> {
    check_rules(id, patch)?;
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let module = locked(&mut tx, scope, id).await?;
    // The column keeps microseconds, so a time equal to the stored one once truncated is the
    // same value and writes nothing.
    let unlock_at = match patch.unlock_at {
        Some(at) => at.map(truncate_micros).transpose()?,
        None => module.unlock_at,
    };
    let sequential = patch
        .require_sequential
        .unwrap_or(module.require_sequential);
    let rule = patch.completion_rule.unwrap_or(module.completion_rule);
    let requires = patch
        .prerequisite_ids
        .as_ref()
        .filter(|ids| !same_set(&module.prerequisite_ids, ids));
    let same = unlock_at == module.unlock_at
        && sequential == module.require_sequential
        && rule == module.completion_rule;
    if same && requires.is_none() {
        return Ok(module);
    }
    if let Some(requires) = requires {
        replace_prerequisites(&mut tx, scope, &module, requires, now).await?;
    }
    sqlx::query!(
        "UPDATE modules
         SET unlock_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond',
             require_sequential = $4, completion_rule = $5,
             updated_at = timestamptz 'epoch' + $6::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
        unlock_at.map(epoch_micros).transpose()?,
        sequential,
        rule.as_str(),
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

/// The checks of [`set_rules`] that need no database.
fn check_rules(id: Uuid, patch: &RulesPatch) -> Result<(), Error> {
    if let Some(Some(at)) = patch.unlock_at
        && !(UNIX_EPOCH..dates_end()).contains(&at)
    {
        return Err(Error::UnlockAtOutOfRange);
    }
    let Some(ids) = &patch.prerequisite_ids else {
        return Ok(());
    };
    if ids.contains(&id) {
        return Err(Error::PrerequisiteSelf);
    }
    if ids.len() > crate::rules::MAX_PREREQUISITES {
        return Err(Error::PrerequisitesTooMany);
    }
    let mut seen = std::collections::HashSet::new();
    if !ids.iter().all(|id| seen.insert(id)) {
        return Err(Error::PrerequisiteDuplicate);
    }
    Ok(())
}

/// Replace the prerequisites of `module` with `requires`, after checking that each is a module
/// of the same course and that the new set closes no cycle. The caller holds the course lock.
async fn replace_prerequisites(
    conn: &mut PgConnection,
    scope: OrgScope,
    module: &Module,
    requires: &[Uuid],
    now: i64,
) -> Result<(), Error> {
    let found = sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM modules
           WHERE org_id = $1 AND course_id = $2 AND id = ANY($3::uuid[])"#,
        scope.org_id(),
        module.course_id,
        requires,
    )
    .fetch_one(&mut *conn)
    .await?;
    if usize::try_from(found).ok() != Some(requires.len()) {
        return Err(Error::PrerequisiteNotFound);
    }
    let edges: Vec<(Uuid, Uuid)> = sqlx::query!(
        "SELECT module_id, requires_id FROM module_prerequisites
         WHERE org_id = $1 AND course_id = $2 AND module_id <> $3",
        scope.org_id(),
        module.course_id,
        module.id,
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|edge| (edge.module_id, edge.requires_id))
    .collect();
    if crate::rules::closes_cycle(&edges, module.id, requires) {
        return Err(Error::PrerequisiteCycle);
    }
    sqlx::query!(
        "DELETE FROM module_prerequisites WHERE org_id = $1 AND module_id = $2",
        scope.org_id(),
        module.id,
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        "INSERT INTO module_prerequisites
             (org_id, course_id, module_id, requires_id, created_at, updated_at)
         SELECT $1, $2, $3, r,
                timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
         FROM unnest($4::uuid[]) AS r",
        scope.org_id(),
        module.course_id,
        module.id,
        requires,
        now,
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// `time` truncated to whole microseconds, the resolution of `timestamptz`.
fn truncate_micros(time: SystemTime) -> Result<SystemTime, Error> {
    Ok(from_epoch_micros(epoch_micros(time)?)?)
}

/// The first instant an unlock date cannot be: the course dates' bound.
fn dates_end() -> SystemTime {
    UNIX_EPOCH + std::time::Duration::from_secs(crate::courses::DATES_END_SECS)
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
