// SPDX-License-Identifier: AGPL-3.0-or-later
//! Learner progress (docs/specs/m1/05-progress-and-events.md B1 to B7, B10 to B16).
//!
//! One `item_progress` row per learner and item: `started` at the learner's first step, then
//! `completed` once the item's requirement is met. Completion is final (Q1): nothing here moves
//! a row back, and a later change to the requirement leaves a completed row as it is.
//!
//! [`record_step`] takes a learner's step (`view` or `mark_done`, B2, B3); a step that meets the
//! item's requirement goes on to [`complete_item`], the one function every way of completing an
//! item goes through (B5): spec 09's watch reports, M2's submissions and M3's mastery call it
//! too. It locks the learner's `course_progress` row, so two completions of one learner in one
//! course take turns, and in the same transaction rolls the change up to the item's module and
//! the course (B6, B7) with `stepclimb_core::unlock`'s definitions (spec 04 B11). It returns the
//! events of what became complete ([`Event`], B10, B11); the caller writes them to the outbox in
//! the same transaction (`stepclimb_jobs::outbox::emit`, which this crate cannot reach).
//!
//! Both functions take the caller's transaction and, before anything else, read the course row
//! `FOR SHARE` (`courses::lock_shared`). Whether the caller may see the course at all (a draft or
//! a template needs `course.view_all`) is the route's to check, on its own `lock_shared` read in
//! the same transaction. Every content write (spec 03 B12), status change and enrollment write locks that
//! row `FOR NO KEY UPDATE`, so while a step runs the course cannot be archived, its rules and
//! items cannot change, an item cannot be deleted (B16) and the learner's enrollment cannot end.
//! Under that lock they check, in this order: the course is not archived (`course.archived`),
//! the user is a learner, holding an active enrollment whose role has `course.participate`
//! (B1, B12), and the item is published, in a published module, and unlocked for them (spec 04
//! B13). Steps of different learners never wait for each other.
//!
//! Staff read every learner's progress in a course through [`list_for_course`] and
//! [`learner_in_course`] (B14); who may read whom (B20) is the routes' to decide, and they pass
//! the users an Observer may see as [`LearnerFilter::only`]. A learner is a user holding an
//! enrollment in the course, active or concluded, whose role has `course.participate`, and who
//! is not suspended.
//!
//! Every query filters by `org_id = scope.org_id()`.

use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

use sqlx::{PgConnection, PgExecutor, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    OrgScope, courses,
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    enrollments,
    items::{self, Kind},
    modules,
    perm::{CourseParticipate, Permission},
    roles,
    rules::{Requirement, RequirementKind},
    unlock::{self, CourseRules, Evaluation, LockReason, ModuleRules},
};

/// Where a learner is with an item (B3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Started,
    Completed,
}

impl State {
    /// The stored and API name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Completed => "completed",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "started" => Some(Self::Started),
            "completed" => Some(Self::Completed),
            _ => None,
        }
    }
}

/// A learner's progress on one item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemProgress {
    pub item_id: Uuid,
    pub state: State,
    pub started_at: SystemTime,
    /// Set exactly when `state` is [`State::Completed`].
    pub completed_at: Option<SystemTime>,
    pub updated_at: SystemTime,
}

/// A learner's roll-up for one module (B6, B7): its published items with a requirement, and how
/// many of them the learner completed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleProgress {
    pub module_id: Uuid,
    pub required_count: i32,
    pub completed_count: i32,
    /// When the module became complete; final once set (Q1).
    pub completed_at: Option<SystemTime>,
}

/// A learner's roll-up for a course (B6, B7): its counted modules (spec 04 B11: published, with
/// a published requirement), and how many of them are complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CourseProgress {
    pub required_count: i32,
    pub completed_count: i32,
    /// When the course became complete; final once set (Q1).
    pub completed_at: Option<SystemTime>,
}

/// Everything stored of one learner's progress in one course (B13).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LearnerProgress {
    /// `None` until the learner first completes an item of the course.
    pub course: Option<CourseProgress>,
    /// One per module the learner completed an item of, in course order.
    pub modules: Vec<ModuleProgress>,
    /// One per item the learner took a step on, in course and module order.
    pub items: Vec<ItemProgress>,
}

/// A step a learner reports (B2, Q3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// They opened a page, a file or an external link.
    View,
    /// They marked an item with a `mark_done` requirement done.
    MarkDone,
}

/// What [`complete_item`] is told the learner did: the kind of requirement they met. It must be
/// the item's current requirement's kind. M2 adds the score.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Completion {
    pub kind: RequirementKind,
}

/// Something that became complete for a learner (B10). Each is reported once per learner and
/// target: only when its `completed_at` goes from null to a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    ItemCompleted(ItemCompleted),
    ModuleCompleted(ModuleCompleted),
    CourseCompleted(CourseCompleted),
}

/// `item.completed` (B11).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemCompleted {
    pub user_id: Uuid,
    pub course_id: Uuid,
    pub module_id: Uuid,
    pub item_id: Uuid,
    pub item_kind: Kind,
    pub requirement_kind: RequirementKind,
    pub completed_at: SystemTime,
}

/// `module.completed` (B11).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleCompleted {
    pub user_id: Uuid,
    pub course_id: Uuid,
    pub module_id: Uuid,
    pub completed_at: SystemTime,
}

/// `course.completed` (B11).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CourseCompleted {
    pub user_id: Uuid,
    pub course_id: Uuid,
    pub completed_at: SystemTime,
}

/// What a step or a completion left: the item's progress, and the events to emit in the same
/// transaction, in the order item, module, course. Empty when nothing became complete.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use = "the events must be written to the outbox in the same transaction"]
pub struct Recorded {
    pub progress: ItemProgress,
    pub events: Vec<Event>,
}

/// Why a progress write was refused.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No published item with that id in a published module of the scope's organization.
    #[error("the item does not exist")]
    ItemNotFound,
    /// The item is locked for the learner, for these reasons (never empty).
    #[error("the item is locked")]
    Locked(Vec<LockReason>),
    /// The course is archived and takes no progress (spec 02 B5).
    #[error("the course is archived")]
    CourseArchived,
    /// The user holds no active enrollment in the course whose role has `course.participate`.
    #[error("the user is not a learner of the course")]
    NotLearner,
    /// The item does not take this step (B2): `view` on a kind other than a page, a file or an
    /// external link, or `mark_done` on an item whose requirement is not `mark_done`.
    #[error("the item does not take this step")]
    ActionInvalid,
    /// [`complete_item`] was told of a requirement kind that is not the item's: a bug in the
    /// caller, since the requirement cannot change while the course row is held.
    #[error("the item's requirement is not the one the completion met")]
    CompletionMismatch,
    /// A stored state this release cannot read, which the table's check rules out.
    #[error("the {column} of the progress of item {item_id} cannot be read")]
    Stored { item_id: Uuid, column: &'static str },
    /// A course's rules or an item could not be read.
    #[error(transparent)]
    Content(#[from] modules::Error),
    /// The course row could not be read.
    #[error(transparent)]
    Course(#[from] courses::Error),
    /// A list cursor whose display name holds NUL, which no stored name can.
    #[error("the cursor is not one this list wrote")]
    InvalidCursor,
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl From<unlock::RequireError> for Error {
    fn from(error: unlock::RequireError) -> Self {
        match error {
            unlock::RequireError::NotFound => Self::ItemNotFound,
            unlock::RequireError::Locked(reasons) => Self::Locked(reasons),
            unlock::RequireError::Repository(error) => Self::Content(error),
        }
    }
}

/// Whether the item `item` takes the step `action` (B2).
#[must_use]
pub fn accepts(item: &items::Item, action: Action) -> bool {
    match action {
        Action::View => matches!(item.kind, Kind::Page | Kind::File | Kind::ExternalLink),
        Action::MarkDone => item.requirement == Some(Requirement::MarkDone),
    }
}

/// The requirement kind `action` meets on `item`, if any: a `view` meets `view`, a `mark_done`
/// meets `mark_done` (B3).
fn meets(item: &items::Item, action: Action) -> Option<RequirementKind> {
    match (action, item.requirement) {
        (Action::View, Some(Requirement::View)) => Some(RequirementKind::View),
        (Action::MarkDone, Some(Requirement::MarkDone)) => Some(RequirementKind::MarkDone),
        _ => None,
    }
}

/// Record the step `action` of the learner `user_id` on the item `item_id` of the scope's
/// organization at `now` (B2, B3), in `tx`.
///
/// Refuses as the module docs say, then refuses a step the item does not take
/// ([`Error::ActionInvalid`]). A step that meets the item's requirement completes it through
/// [`complete_item`]'s path. Any other step creates the `started` row if there is none; a step
/// that changes nothing writes nothing and returns the progress as it is.
pub async fn record_step(
    tx: &mut Transaction<'_, Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    item_id: Uuid,
    action: Action,
    now: SystemTime,
) -> Result<Recorded, Error> {
    let item = prepare(tx, scope, user_id, item_id).await?;
    if accepts(&item, action)
        && let Some(kind) = meets(&item, action)
    {
        return complete_prepared(tx, scope, user_id, &item, kind, now).await;
    }
    // Every other step checks the lock before the step itself, as the completing path does.
    let completed = completed_items(&mut **tx, scope, user_id, item.course_id).await?;
    let rules = unlock::load_course_rules(&mut **tx, scope, item.course_id).await?;
    unlock::check_item(&unlock::evaluate(&rules, &completed, now), item.id)?;
    if !accepts(&item, action) {
        return Err(Error::ActionInvalid);
    }
    let progress = start(tx, scope, user_id, &item, now).await?;
    Ok(Recorded {
        progress,
        events: Vec::new(),
    })
}

/// Complete the item `item_id` of the scope's organization for the learner `user_id` at `now`,
/// who met `completion`, in `tx` (B5, B6). The one way an item becomes complete.
///
/// Refuses as the module docs say, and with [`Error::CompletionMismatch`] when the item's
/// requirement is not of `completion`'s kind. An item already complete writes nothing and
/// returns no event (B4, B10). Otherwise the row becomes `completed`, the learner's module and
/// course roll-ups are recomputed under the `course_progress` row's lock, and the events of
/// what became complete are returned.
pub async fn complete_item(
    tx: &mut Transaction<'_, Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    item_id: Uuid,
    completion: Completion,
    now: SystemTime,
) -> Result<Recorded, Error> {
    let item = prepare(tx, scope, user_id, item_id).await?;
    if item.requirement.map(Requirement::kind) != Some(completion.kind) {
        return Err(Error::CompletionMismatch);
    }
    complete_prepared(tx, scope, user_id, &item, completion.kind, now).await
}

/// The item, once the course row is held `FOR SHARE`, the course is not archived and the user
/// is a learner of it.
async fn prepare(
    tx: &mut Transaction<'_, Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    item_id: Uuid,
) -> Result<items::Item, Error> {
    let course_id = items::course_of(&mut **tx, scope, item_id)
        .await?
        .ok_or(Error::ItemNotFound)?;
    let course = courses::lock_shared(tx, scope, course_id)
        .await?
        .ok_or(Error::ItemNotFound)?;
    if course.status == courses::Status::Archived {
        return Err(Error::CourseArchived);
    }
    if !enrollments::is_learner(tx, scope, course_id, user_id).await? {
        return Err(Error::NotLearner);
    }
    // The course row held `FOR SHARE` keeps the item from being deleted or moved.
    items::get(&mut **tx, scope, item_id)
        .await?
        .ok_or(Error::ItemNotFound)
}

/// [`complete_item`] for an item [`prepare`] returned, whose requirement is of kind `kind`.
async fn complete_prepared(
    tx: &mut Transaction<'_, Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    item: &items::Item,
    kind: RequirementKind,
    now: SystemTime,
) -> Result<Recorded, Error> {
    let course_before = lock_course_progress(tx, scope, user_id, item.course_id).await?;
    let mut completed = completed_items(&mut **tx, scope, user_id, item.course_id).await?;
    let rules = unlock::load_course_rules(&mut **tx, scope, item.course_id).await?;
    unlock::check_item(&unlock::evaluate(&rules, &completed, now), item.id)?;
    if let Some(progress) = item_progress(&mut **tx, scope, user_id, item.id).await?
        && progress.state == State::Completed
    {
        return Ok(Recorded {
            progress,
            events: Vec::new(),
        });
    }
    let progress = mark_completed(tx, scope, user_id, item, now).await?;
    completed.insert(item.id);
    let evaluation = unlock::evaluate(&rules, &completed, now);
    let mut events = vec![Event::ItemCompleted(ItemCompleted {
        user_id,
        course_id: item.course_id,
        module_id: item.module_id,
        item_id: item.id,
        item_kind: item.kind,
        requirement_kind: kind,
        completed_at: now,
    })];
    let rollup = RollUp {
        scope,
        user_id,
        course_id: item.course_id,
        now,
    };
    // `check_item` passed, so the item is in a published module of `rules`.
    if let Some(module) = rules.modules.iter().find(|m| holds(m, item.id)) {
        events.extend(rollup.module(tx, module, &completed, &evaluation).await?);
    }
    events.extend(
        rollup
            .course(tx, &rules, &evaluation, course_before)
            .await?,
    );
    Ok(Recorded { progress, events })
}

fn holds(module: &ModuleRules, item_id: Uuid) -> bool {
    module.items.iter().any(|item| item.id == item_id)
}

/// The learner and course a roll-up writes for.
struct RollUp {
    scope: OrgScope,
    user_id: Uuid,
    course_id: Uuid,
    now: SystemTime,
}

impl RollUp {
    /// Store `module`'s counts for the learner who completed `completed`, and its
    /// `completed_at` when `evaluation` finds it complete for the first time: then its event.
    async fn module(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        module: &ModuleRules,
        completed: &HashSet<Uuid>,
        evaluation: &Evaluation,
    ) -> Result<Option<Event>, Error> {
        let required: Vec<Uuid> = module
            .items
            .iter()
            .filter(|item| item.requirement.is_some())
            .map(|item| item.id)
            .collect();
        let done = required.iter().filter(|id| completed.contains(id)).count();
        let complete = evaluation.module(module.id).is_some_and(|m| m.complete);
        let before = sqlx::query_scalar!(
            r#"SELECT completed_at IS NOT NULL AS "complete!" FROM module_progress
               WHERE org_id = $1 AND user_id = $2 AND module_id = $3"#,
            self.scope.org_id(),
            self.user_id,
            module.id,
        )
        .fetch_optional(&mut **tx)
        .await?
        .unwrap_or(false);
        let newly = complete && !before;
        sqlx::query!(
            "INSERT INTO module_progress
                 (org_id, user_id, course_id, module_id, required_count, completed_count,
                  completed_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6,
                     CASE WHEN $7 THEN timestamptz 'epoch' + $8::bigint * interval '1 microsecond' END,
                     timestamptz 'epoch' + $8::bigint * interval '1 microsecond',
                     timestamptz 'epoch' + $8::bigint * interval '1 microsecond')
             ON CONFLICT (user_id, module_id) DO UPDATE
             SET required_count = EXCLUDED.required_count,
                 completed_count = EXCLUDED.completed_count,
                 completed_at = coalesce(module_progress.completed_at, EXCLUDED.completed_at),
                 updated_at = EXCLUDED.updated_at",
            self.scope.org_id(),
            self.user_id,
            self.course_id,
            module.id,
            count(required.len()),
            count(done),
            newly,
            epoch_micros(self.now)?,
        )
        .execute(&mut **tx)
        .await?;
        Ok(newly.then_some(Event::ModuleCompleted(ModuleCompleted {
            user_id: self.user_id,
            course_id: self.course_id,
            module_id: module.id,
            completed_at: self.now,
        })))
    }

    /// Store the course's counts over its counted modules (spec 04 B11), and its `completed_at`
    /// when `evaluation` finds it complete and `before` (the locked row's) is null: then its
    /// event.
    async fn course(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        rules: &CourseRules,
        evaluation: &Evaluation,
        before: Option<SystemTime>,
    ) -> Result<Option<Event>, Error> {
        let counted: Vec<Uuid> = rules
            .modules
            .iter()
            .filter(|m| m.items.iter().any(|item| item.requirement.is_some()))
            .map(|m| m.id)
            .collect();
        let done = counted
            .iter()
            .filter(|id| evaluation.module(**id).is_some_and(|m| m.complete))
            .count();
        let newly = evaluation.course_complete() && before.is_none();
        sqlx::query!(
            "UPDATE course_progress
             SET required_count = $4, completed_count = $5,
                 completed_at = CASE WHEN $6 THEN timestamptz 'epoch' + $7::bigint * interval '1 microsecond'
                                     ELSE completed_at END,
                 updated_at = timestamptz 'epoch' + $7::bigint * interval '1 microsecond'
             WHERE org_id = $1 AND user_id = $2 AND course_id = $3",
            self.scope.org_id(),
            self.user_id,
            self.course_id,
            count(counted.len()),
            count(done),
            newly,
            epoch_micros(self.now)?,
        )
        .execute(&mut **tx)
        .await?;
        Ok(newly.then_some(Event::CourseCompleted(CourseCompleted {
            user_id: self.user_id,
            course_id: self.course_id,
            completed_at: self.now,
        })))
    }
}

/// A count as the `integer` columns store it.
fn count(n: usize) -> i32 {
    // Invariant: a count is of a course's modules or one module's items, at most 200 each
    // (spec 03 B1, B4), which every `i32` holds.
    i32::try_from(n).expect("a count of at most 200")
}

/// Lock the learner's `course_progress` row for the course, creating it with zero counts when
/// absent, and return its `completed_at` (B6). The row stays locked until `tx` ends, so the
/// learner's completions in the course take turns.
async fn lock_course_progress(
    tx: &mut Transaction<'_, Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    course_id: Uuid,
) -> Result<Option<SystemTime>, Error> {
    sqlx::query!(
        "INSERT INTO course_progress (org_id, user_id, course_id, required_count, completed_count)
         VALUES ($1, $2, $3, 0, 0)
         ON CONFLICT (user_id, course_id) DO NOTHING",
        scope.org_id(),
        user_id,
        course_id,
    )
    .execute(&mut **tx)
    .await?;
    let completed_at = sqlx::query_scalar!(
        r#"SELECT (extract(epoch FROM completed_at) * 1000000)::bigint AS completed_at_us
           FROM course_progress
           WHERE org_id = $1 AND user_id = $2 AND course_id = $3
           FOR UPDATE"#,
        scope.org_id(),
        user_id,
        course_id,
    )
    .fetch_one(&mut **tx)
    .await?;
    Ok(completed_at.map(from_epoch_micros).transpose()?)
}

/// The ids of the items of the course the learner has completed.
async fn completed_items(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: Uuid,
    course_id: Uuid,
) -> Result<HashSet<Uuid>, Error> {
    let ids = sqlx::query_scalar!(
        "SELECT item_id FROM item_progress
         WHERE org_id = $1 AND course_id = $2 AND user_id = $3 AND state = 'completed'",
        scope.org_id(),
        course_id,
        user_id,
    )
    .fetch_all(db)
    .await?;
    Ok(ids.into_iter().collect())
}

struct ItemRow {
    item_id: Uuid,
    state: String,
    started_at_us: i64,
    completed_at_us: Option<i64>,
    updated_at_us: i64,
}

impl TryFrom<ItemRow> for ItemProgress {
    type Error = Error;

    fn try_from(row: ItemRow) -> Result<Self, Error> {
        Ok(Self {
            state: State::parse(&row.state).ok_or(Error::Stored {
                item_id: row.item_id,
                column: "state",
            })?,
            started_at: from_epoch_micros(row.started_at_us)?,
            completed_at: row.completed_at_us.map(from_epoch_micros).transpose()?,
            updated_at: from_epoch_micros(row.updated_at_us)?,
            item_id: row.item_id,
        })
    }
}

async fn item_progress(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: Uuid,
    item_id: Uuid,
) -> Result<Option<ItemProgress>, Error> {
    sqlx::query_as!(
        ItemRow,
        r#"SELECT item_id, state,
                  (extract(epoch FROM started_at) * 1000000)::bigint AS "started_at_us!",
                  (extract(epoch FROM completed_at) * 1000000)::bigint AS completed_at_us,
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM item_progress WHERE org_id = $1 AND user_id = $2 AND item_id = $3"#,
        scope.org_id(),
        user_id,
        item_id,
    )
    .fetch_optional(db)
    .await?
    .map(ItemProgress::try_from)
    .transpose()
}

/// The learner's row for `item`, inserted `started` at `now` when there is none. A row another
/// step of the same learner wrote meanwhile is kept as it is and returned.
async fn start(
    tx: &mut Transaction<'_, Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    item: &items::Item,
    now: SystemTime,
) -> Result<ItemProgress, Error> {
    sqlx::query!(
        "INSERT INTO item_progress
             (org_id, user_id, course_id, item_id, state, started_at, created_at, updated_at)
         VALUES ($1, $2, $3, $4, 'started',
                 timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $5::bigint * interval '1 microsecond')
         ON CONFLICT (user_id, item_id) DO NOTHING",
        scope.org_id(),
        user_id,
        item.course_id,
        item.id,
        epoch_micros(now)?,
    )
    .execute(&mut **tx)
    .await?;
    item_progress(&mut **tx, scope, user_id, item.id)
        .await?
        .ok_or(Error::ItemNotFound)
}

/// Make the learner's row for `item` `completed` at `now`, inserting it when there is none
/// (then `started_at` is `now` too). The caller found no completed row under the
/// `course_progress` lock.
async fn mark_completed(
    tx: &mut Transaction<'_, Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    item: &items::Item,
    now: SystemTime,
) -> Result<ItemProgress, Error> {
    sqlx::query_as!(
        ItemRow,
        r#"INSERT INTO item_progress
               (org_id, user_id, course_id, item_id, state, started_at, completed_at,
                created_at, updated_at)
           VALUES ($1, $2, $3, $4, 'completed',
                   timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                   timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                   timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                   timestamptz 'epoch' + $5::bigint * interval '1 microsecond')
           ON CONFLICT (user_id, item_id) DO UPDATE
           SET state = 'completed', completed_at = EXCLUDED.completed_at,
               updated_at = EXCLUDED.updated_at
           RETURNING item_id, state,
                     (extract(epoch FROM started_at) * 1000000)::bigint AS "started_at_us!",
                     (extract(epoch FROM completed_at) * 1000000)::bigint AS completed_at_us,
                     (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!""#,
        scope.org_id(),
        user_id,
        item.course_id,
        item.id,
        epoch_micros(now)?,
    )
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}

/// Everything stored of the learner `user_id`'s progress in the course `course_id` of the
/// scope's organization (B13): what the learner's own course reads show them.
pub async fn for_learner_in_course(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
    course_id: Uuid,
) -> Result<LearnerProgress, Error> {
    let course = sqlx::query!(
        r#"SELECT required_count, completed_count,
                  (extract(epoch FROM completed_at) * 1000000)::bigint AS completed_at_us
           FROM course_progress WHERE org_id = $1 AND user_id = $2 AND course_id = $3"#,
        scope.org_id(),
        user_id,
        course_id,
    )
    .fetch_optional(&mut *conn)
    .await?
    .map(|row| {
        Ok::<_, Error>(CourseProgress {
            required_count: row.required_count,
            completed_count: row.completed_count,
            completed_at: row.completed_at_us.map(from_epoch_micros).transpose()?,
        })
    })
    .transpose()?;
    let modules = sqlx::query!(
        r#"SELECT p.module_id, p.required_count, p.completed_count,
                  (extract(epoch FROM p.completed_at) * 1000000)::bigint AS completed_at_us
           FROM module_progress p
           JOIN modules m ON m.org_id = p.org_id AND m.id = p.module_id
           WHERE p.org_id = $1 AND p.user_id = $2 AND p.course_id = $3
           ORDER BY m.position"#,
        scope.org_id(),
        user_id,
        course_id,
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| {
        Ok(ModuleProgress {
            module_id: row.module_id,
            required_count: row.required_count,
            completed_count: row.completed_count,
            completed_at: row.completed_at_us.map(from_epoch_micros).transpose()?,
        })
    })
    .collect::<Result<_, Error>>()?;
    let items = sqlx::query_as!(
        ItemRow,
        r#"SELECT p.item_id, p.state,
                  (extract(epoch FROM p.started_at) * 1000000)::bigint AS "started_at_us!",
                  (extract(epoch FROM p.completed_at) * 1000000)::bigint AS completed_at_us,
                  (extract(epoch FROM p.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM item_progress p
           JOIN module_items i ON i.org_id = p.org_id AND i.id = p.item_id
           JOIN modules m ON m.org_id = i.org_id AND m.id = i.module_id
           WHERE p.org_id = $1 AND p.user_id = $2 AND p.course_id = $3
           ORDER BY m.position, i.position"#,
        scope.org_id(),
        user_id,
        course_id,
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(ItemProgress::try_from)
    .collect::<Result<_, _>>()?;
    Ok(LearnerProgress {
        course,
        modules,
        items,
    })
}

/// Which of a course's modules and items a staff read shows (B14): a caller without
/// `course.view_all` at the course sees the progress of published content only, so a read
/// never names an unpublished module or item to them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// Published modules, and published items in published modules.
    Published,
    /// Every module and item, published or not.
    All,
}

/// One learner of a course with their stored roll-ups (B14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Learner {
    pub user_id: Uuid,
    pub display_name: String,
    /// `Active` when any of their learner enrollments in the course is, otherwise `Concluded`.
    pub enrollment_status: enrollments::Status,
    /// `None` until the learner first completes an item of the course.
    pub course: Option<CourseProgress>,
    /// One per shown module the learner completed an item of, in course order.
    pub modules: Vec<ModuleProgress>,
}

/// One learner of a course with their item rows as well (B14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LearnerDetail {
    pub learner: Learner,
    /// One per shown item the learner took a step on, in course and module order.
    pub items: Vec<ItemProgress>,
}

/// The filters of [`list_for_course`]; `None` filters nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LearnerFilter {
    /// Over [`Learner::enrollment_status`].
    pub status: Option<enrollments::Status>,
    /// Only these users: an Observer's observed learners (B20).
    pub only: Option<Vec<Uuid>>,
}

/// The sort key of a learner in [`list_for_course`]: the display name, then the user's id.
pub type LearnerKey = (String, Uuid);

/// The learners of the course `course_id` of the scope's organization who are not suspended,
/// matching `filter`, with their course and `shown` module progress, sorted by display name
/// then user id, starting after `after` (the sort key of the previous page's last learner) and at most `limit` of them
/// (B14). `after` comes from a client's cursor and only bounds a query already filtered by the
/// scope; a NUL in its name is [`Error::InvalidCursor`].
pub async fn list_for_course(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    filter: &LearnerFilter,
    shown: Shown,
    after: Option<&LearnerKey>,
    limit: i64,
) -> Result<Vec<Learner>, Error> {
    if after.is_some_and(|(name, _)| name.contains('\0')) {
        return Err(Error::InvalidCursor);
    }
    let learner_roles = roles::ids_holding(&mut *conn, scope, CourseParticipate::KEY).await?;
    let rows = sqlx::query!(
        r#"SELECT u.id AS "user_id!", u.display_name AS "display_name!",
                  bool_or(e.status = 'active') AS "active!"
           FROM enrollments e JOIN users u ON u.org_id = e.org_id AND u.id = e.user_id
           WHERE e.org_id = $1 AND e.course_id = $2 AND e.role_id = ANY($3)
             AND u.status = 'active'
             AND ($4::uuid[] IS NULL OR e.user_id = ANY($4))
           GROUP BY u.id, u.display_name
           HAVING ($5::text IS NULL
                   OR CASE WHEN bool_or(e.status = 'active') THEN 'active' ELSE 'concluded' END = $5)
              AND ($6::text IS NULL OR (u.display_name, u.id) > ($6, $7::uuid))
           ORDER BY u.display_name, u.id
           LIMIT $8"#,
        scope.org_id(),
        course_id,
        &learner_roles,
        filter.only.as_deref(),
        filter.status.map(enrollments::Status::as_str),
        after.map(|(name, _)| name.as_str()),
        after.map(|(_, id)| *id),
        limit,
    )
    .fetch_all(&mut *conn)
    .await?;
    let users: Vec<Uuid> = rows.iter().map(|row| row.user_id).collect();
    let mut courses = course_rows(&mut *conn, scope, course_id, &users).await?;
    let mut modules = module_rows(&mut *conn, scope, course_id, &users, shown).await?;
    Ok(rows
        .into_iter()
        .map(|row| Learner {
            course: courses.remove(&row.user_id),
            modules: modules.remove(&row.user_id).unwrap_or_default(),
            user_id: row.user_id,
            display_name: row.display_name,
            enrollment_status: if row.active {
                enrollments::Status::Active
            } else {
                enrollments::Status::Concluded
            },
        })
        .collect())
}

/// The learner `user_id` of the course `course_id` of the scope's organization with their
/// course, `shown` module and `shown` item progress, or `None` when they are not a learner of
/// it or are suspended (B14).
pub async fn learner_in_course(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    user_id: Uuid,
    shown: Shown,
) -> Result<Option<LearnerDetail>, Error> {
    let filter = LearnerFilter {
        status: None,
        only: Some(vec![user_id]),
    };
    let found = list_for_course(conn, scope, course_id, &filter, shown, None, 1).await?;
    let Some(learner) = found.into_iter().next() else {
        return Ok(None);
    };
    let items = sqlx::query_as!(
        ItemRow,
        r#"SELECT p.item_id, p.state,
                  (extract(epoch FROM p.started_at) * 1000000)::bigint AS "started_at_us!",
                  (extract(epoch FROM p.completed_at) * 1000000)::bigint AS completed_at_us,
                  (extract(epoch FROM p.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM item_progress p
           JOIN module_items i ON i.org_id = p.org_id AND i.id = p.item_id
           JOIN modules m ON m.org_id = i.org_id AND m.id = i.module_id
           WHERE p.org_id = $1 AND p.user_id = $2 AND p.course_id = $3
             AND ($4 OR (i.published AND m.published))
           ORDER BY m.position, i.position"#,
        scope.org_id(),
        user_id,
        course_id,
        shown == Shown::All,
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(ItemProgress::try_from)
    .collect::<Result<_, _>>()?;
    Ok(Some(LearnerDetail { learner, items }))
}

/// The `course_progress` rows of `users` in the course, by user.
async fn course_rows(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    users: &[Uuid],
) -> Result<HashMap<Uuid, CourseProgress>, Error> {
    let rows = sqlx::query!(
        r#"SELECT user_id, required_count, completed_count,
                  (extract(epoch FROM completed_at) * 1000000)::bigint AS completed_at_us
           FROM course_progress
           WHERE org_id = $1 AND course_id = $2 AND user_id = ANY($3)"#,
        scope.org_id(),
        course_id,
        users,
    )
    .fetch_all(conn)
    .await?;
    rows.into_iter()
        .map(|row| {
            let progress = CourseProgress {
                required_count: row.required_count,
                completed_count: row.completed_count,
                completed_at: row.completed_at_us.map(from_epoch_micros).transpose()?,
            };
            Ok((row.user_id, progress))
        })
        .collect()
}

/// The `module_progress` rows of `users` in the course for `shown` modules, by user, each in
/// course order.
async fn module_rows(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    users: &[Uuid],
    shown: Shown,
) -> Result<HashMap<Uuid, Vec<ModuleProgress>>, Error> {
    let rows = sqlx::query!(
        r#"SELECT p.user_id, p.module_id, p.required_count, p.completed_count,
                  (extract(epoch FROM p.completed_at) * 1000000)::bigint AS completed_at_us
           FROM module_progress p
           JOIN modules m ON m.org_id = p.org_id AND m.id = p.module_id
           WHERE p.org_id = $1 AND p.course_id = $2 AND p.user_id = ANY($3)
             AND ($4 OR m.published)
           ORDER BY m.position"#,
        scope.org_id(),
        course_id,
        users,
        shown == Shown::All,
    )
    .fetch_all(conn)
    .await?;
    let mut by_user: HashMap<Uuid, Vec<ModuleProgress>> = HashMap::new();
    for row in rows {
        by_user
            .entry(row.user_id)
            .or_default()
            .push(ModuleProgress {
                module_id: row.module_id,
                required_count: row.required_count,
                completed_count: row.completed_count,
                completed_at: row.completed_at_us.map(from_epoch_micros).transpose()?,
            });
    }
    Ok(by_user)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: Kind, requirement: Option<Requirement>) -> items::Item {
        items::Item {
            id: Uuid::from_u128(1),
            module_id: Uuid::from_u128(2),
            course_id: Uuid::from_u128(3),
            kind,
            title: "Item".into(),
            position: 0,
            published: true,
            ref_id: None,
            url: None,
            requirement,
            created_at: SystemTime::UNIX_EPOCH,
            updated_at: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn b2_view_is_taken_by_pages_files_and_links_only() {
        for kind in [Kind::Page, Kind::File, Kind::ExternalLink] {
            assert!(accepts(&item(kind, None), Action::View), "{kind:?}");
        }
        for kind in [Kind::Header, Kind::Video, Kind::Quiz, Kind::Activity] {
            assert!(!accepts(&item(kind, None), Action::View), "{kind:?}");
        }
    }

    #[test]
    fn b2_mark_done_is_taken_only_under_a_mark_done_requirement() {
        let watch = Requirement::Watch { min_percent: 50 };
        assert!(accepts(
            &item(Kind::Video, Some(Requirement::MarkDone)),
            Action::MarkDone
        ));
        for requirement in [None, Some(Requirement::View), Some(watch)] {
            let item = item(Kind::Page, requirement);
            assert!(!accepts(&item, Action::MarkDone), "{requirement:?}");
        }
    }

    #[test]
    fn b3_a_step_meets_only_its_own_requirement() {
        let view = item(Kind::Page, Some(Requirement::View));
        let done = item(Kind::Page, Some(Requirement::MarkDone));
        let plain = item(Kind::Page, None);
        assert_eq!(meets(&view, Action::View), Some(RequirementKind::View));
        assert_eq!(meets(&done, Action::View), None);
        assert_eq!(
            meets(&done, Action::MarkDone),
            Some(RequirementKind::MarkDone)
        );
        assert_eq!(meets(&plain, Action::View), None);
        assert_eq!(meets(&view, Action::MarkDone), None);
    }

    #[test]
    fn states_round_trip_their_names() {
        for state in [State::Started, State::Completed] {
            assert_eq!(State::parse(state.as_str()), Some(state));
        }
        assert_eq!(State::parse("done"), None);
    }
}
