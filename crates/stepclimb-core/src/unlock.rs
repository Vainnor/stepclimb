// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unlock and completion (docs/specs/m1/04-unlock-and-completion-rules.md B4, B6 to B11, B13).
//!
//! [`evaluate`] decides, for one learner, which modules and items of a course are locked and
//! why, and which are complete. It is a pure function of three inputs: the course's published
//! rule data ([`CourseRules`], read by [`load_course_rules`]), the ids of the items the learner
//! has completed (spec 05's progress rows), and the time from the request's
//! [`crate::clock::Clock`]. It reads no database and no clock of its own, and nothing is stored
//! when an unlock date passes (B10).
//!
//! [`require_unlocked`] is the check every route that hands a learner an item's content or
//! accepts progress for it runs first (B13). Whether a caller is held to the lock at all
//! (`perm::CourseViewAll`, B5) is the route's to decide.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::SystemTime;

use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use crate::{
    OrgScope,
    db::from_epoch_micros,
    items,
    modules::Error,
    rules::{CompletionRule, Requirement},
};

/// A course's published modules in course order, each with its published items in module
/// order (B6: unpublished modules and items take no part, so they are not here).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CourseRules {
    pub modules: Vec<ModuleRules>,
}

/// One published module's rules (B1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleRules {
    pub id: Uuid,
    pub unlock_at: Option<SystemTime>,
    /// The modules it waits for. An id that is not a module of the [`CourseRules`] names an
    /// unpublished module and counts as met (B6).
    pub prerequisite_ids: Vec<Uuid>,
    pub require_sequential: bool,
    pub completion_rule: CompletionRule,
    pub items: Vec<ItemRules>,
}

/// One published item's rules (B3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemRules {
    pub id: Uuid,
    pub requirement: Option<Requirement>,
}

/// One condition that keeps a module or item locked (B8, B9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LockReason {
    /// The module opens at `at`.
    UnlockAt { at: SystemTime },
    /// The module waits for these published modules, in course order.
    Prerequisites { module_ids: Vec<Uuid> },
    /// The item's module is locked.
    ModuleLocked,
    /// The item waits for this earlier item of its sequential module.
    Sequential { item_id: Uuid },
}

/// Whether a module or item is open to the learner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Available,
    /// Locked, with every reason that holds; never empty.
    Locked(Vec<LockReason>),
}

impl State {
    fn from_reasons(reasons: Vec<LockReason>) -> Self {
        if reasons.is_empty() {
            Self::Available
        } else {
            Self::Locked(reasons)
        }
    }

    #[must_use]
    pub fn is_locked(&self) -> bool {
        matches!(self, Self::Locked(_))
    }

    /// The reasons it is locked; empty when it is available.
    #[must_use]
    pub fn reasons(&self) -> &[LockReason] {
        match self {
            Self::Available => &[],
            Self::Locked(reasons) => reasons,
        }
    }
}

/// A module's lock state and completion for one learner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleState {
    pub state: State,
    /// B11: its requirements are complete as its completion rule asks. Never true for a module
    /// without a published requirement (Q4).
    pub complete: bool,
}

/// An item's lock state and completion for one learner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemState {
    pub state: State,
    /// B11: the learner's progress row for it is completed.
    pub complete: bool,
}

/// What [`evaluate`] decided for every published module and item of a course.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Evaluation {
    modules: BTreeMap<Uuid, ModuleState>,
    items: BTreeMap<Uuid, ItemState>,
    course_complete: bool,
}

impl Evaluation {
    /// The state of the published module `id`, or `None` for any other id.
    #[must_use]
    pub fn module(&self, id: Uuid) -> Option<&ModuleState> {
        self.modules.get(&id)
    }

    /// The state of the published item `id` of a published module, or `None` for any other id.
    #[must_use]
    pub fn item(&self, id: Uuid) -> Option<&ItemState> {
        self.items.get(&id)
    }

    /// B11: every published module with a published requirement is complete, and there is at
    /// least one such module.
    #[must_use]
    pub fn course_complete(&self) -> bool {
        self.course_complete
    }
}

/// The lock state and completion of every module and item of `rules` for a learner who has
/// completed the items `completed`, at the time `now` (B7 to B11).
///
/// A completed item stays complete whatever its requirement now says (B4): completion is read
/// from `completed` alone. An `unlock_at` equal to `now` is open (T1).
#[must_use]
pub fn evaluate(rules: &CourseRules, completed: &HashSet<Uuid>, now: SystemTime) -> Evaluation {
    let mut course = Course::new(rules, completed, now);
    let mut evaluation = Evaluation::default();
    for index in 0..rules.modules.len() {
        let module = &rules.modules[index];
        let state = State::from_reasons(course.reasons(index));
        evaluation
            .items
            .extend(item_states(module, &state, completed));
        evaluation.modules.insert(
            module.id,
            ModuleState {
                state,
                complete: course.complete[index],
            },
        );
    }
    let counted: Vec<usize> = (0..rules.modules.len())
        .filter(|&index| course.has_requirement[index])
        .collect();
    evaluation.course_complete =
        !counted.is_empty() && counted.iter().all(|&index| course.complete[index]);
    evaluation
}

/// Whether a module's reasons are still being worked out, or what they are.
#[derive(Clone)]
enum Visit {
    Unvisited,
    InProgress,
    Done(Vec<LockReason>),
}

/// The per-module facts [`evaluate`] works from, and the module reasons found so far.
struct Course<'a> {
    rules: &'a CourseRules,
    now: SystemTime,
    index: HashMap<Uuid, usize>,
    has_requirement: Vec<bool>,
    complete: Vec<bool>,
    visits: Vec<Visit>,
}

impl<'a> Course<'a> {
    fn new(rules: &'a CourseRules, completed: &HashSet<Uuid>, now: SystemTime) -> Self {
        let modules = &rules.modules;
        Self {
            rules,
            now,
            index: modules.iter().enumerate().map(|(i, m)| (m.id, i)).collect(),
            has_requirement: modules
                .iter()
                .map(|m| m.items.iter().any(|item| item.requirement.is_some()))
                .collect(),
            complete: modules
                .iter()
                .map(|m| module_complete(m, completed))
                .collect(),
            visits: vec![Visit::Unvisited; modules.len()],
        }
    }

    /// The reasons the module at `index` is locked (B8): its unlock date, then its unsatisfied
    /// published prerequisites in course order.
    fn reasons(&mut self, index: usize) -> Vec<LockReason> {
        if let Visit::Done(reasons) = &self.visits[index] {
            return reasons.clone();
        }
        self.visits[index] = Visit::InProgress;
        let rules = self.rules;
        let module = &rules.modules[index];
        let mut reasons = Vec::new();
        if let Some(at) = module.unlock_at
            && at > self.now
        {
            reasons.push(LockReason::UnlockAt { at });
        }
        let mut prerequisites: Vec<usize> = module
            .prerequisite_ids
            .iter()
            .filter_map(|id| self.index.get(id).copied())
            .collect();
        prerequisites.sort_unstable();
        prerequisites.dedup();
        let module_ids: Vec<Uuid> = prerequisites
            .into_iter()
            .filter(|&required| !self.satisfied(required))
            .map(|required| rules.modules[required].id)
            .collect();
        if !module_ids.is_empty() {
            reasons.push(LockReason::Prerequisites { module_ids });
        }
        self.visits[index] = Visit::Done(reasons.clone());
        reasons
    }

    /// Whether the module at `index` satisfies a prerequisite on it (B8, Q4): it is complete,
    /// or it has no published requirement and is itself unlocked.
    ///
    /// A module reached again while its own reasons are being worked out is on a cycle, which
    /// `rules::closes_cycle` keeps out of the stored graph. Should one get in, that module
    /// counts as not satisfied, so every module on the cycle that is not complete stays locked,
    /// and the walk ends. A complete module still satisfies a prerequisite on it (B8).
    fn satisfied(&mut self, index: usize) -> bool {
        if self.complete[index] {
            return true;
        }
        if self.has_requirement[index] || matches!(self.visits[index], Visit::InProgress) {
            return false;
        }
        self.reasons(index).is_empty()
    }
}

/// B11: the module's published items with requirements are complete, all of them under `all`
/// and at least one under `one`. A module with none is never complete (Q4).
fn module_complete(module: &ModuleRules, completed: &HashSet<Uuid>) -> bool {
    let mut required = module
        .items
        .iter()
        .filter(|item| item.requirement.is_some())
        .map(|item| completed.contains(&item.id))
        .peekable();
    if required.peek().is_none() {
        return false;
    }
    match module.completion_rule {
        CompletionRule::All => required.all(|done| done),
        CompletionRule::One => required.any(|done| done),
    }
}

/// The items of `module`, whose own state is `module_state` (B9): every item of a locked module
/// is locked by it alone; in a sequential module an item waits for the first earlier item with
/// a requirement that is not complete.
fn item_states(
    module: &ModuleRules,
    module_state: &State,
    completed: &HashSet<Uuid>,
) -> Vec<(Uuid, ItemState)> {
    let mut waiting_for: Option<Uuid> = None;
    let mut states = Vec::with_capacity(module.items.len());
    for item in &module.items {
        let state = if module_state.is_locked() {
            State::Locked(vec![LockReason::ModuleLocked])
        } else if let Some(item_id) = waiting_for {
            State::Locked(vec![LockReason::Sequential { item_id }])
        } else {
            State::Available
        };
        let complete = completed.contains(&item.id);
        if module.require_sequential
            && waiting_for.is_none()
            && item.requirement.is_some()
            && !complete
        {
            waiting_for = Some(item.id);
        }
        states.push((item.id, ItemState { state, complete }));
    }
    states
}

struct Row {
    module_id: Uuid,
    unlock_at_us: Option<i64>,
    prerequisite_ids: Vec<Uuid>,
    require_sequential: bool,
    completion_rule: String,
    item_id: Option<Uuid>,
    requirement: Option<String>,
}

/// The published rule data of the course `course_id` of the scope's organization, which
/// [`evaluate`] takes, in one query. Unpublished modules, unpublished items, the items of
/// unpublished modules, and prerequisites on unpublished modules are left out (B6). A course
/// that does not exist in the organization has no modules.
pub async fn load_course_rules(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    course_id: Uuid,
) -> Result<CourseRules, Error> {
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT m.id AS module_id,
                  (extract(epoch FROM m.unlock_at) * 1000000)::bigint AS unlock_at_us,
                  ARRAY(SELECT p.requires_id
                        FROM module_prerequisites p
                        JOIN modules r ON r.org_id = p.org_id AND r.id = p.requires_id
                        WHERE p.org_id = m.org_id AND p.module_id = m.id AND r.published
                        ORDER BY r.position) AS "prerequisite_ids!",
                  m.require_sequential, m.completion_rule,
                  i.id AS "item_id?", i.requirement::text AS requirement
           FROM modules m
           LEFT JOIN module_items i
                  ON i.org_id = m.org_id AND i.module_id = m.id AND i.published
           WHERE m.org_id = $1 AND m.course_id = $2 AND m.published
           ORDER BY m.position, i.position"#,
        scope.org_id(),
        course_id,
    )
    .fetch_all(db)
    .await?;
    let mut rules = CourseRules::default();
    for row in rows {
        if rules.modules.last().is_none_or(|m| m.id != row.module_id) {
            rules.modules.push(module_rules(&row)?);
        }
        if let Some(id) = row.item_id {
            let requirement = row
                .requirement
                .as_deref()
                .map(|text| stored_requirement(id, text))
                .transpose()?;
            // Invariant: a module was pushed for this row just above, or by an earlier row of
            // the same module.
            let module = rules.modules.last_mut().expect("the row's module");
            module.items.push(ItemRules { id, requirement });
        }
    }
    Ok(rules)
}

fn module_rules(row: &Row) -> Result<ModuleRules, Error> {
    Ok(ModuleRules {
        id: row.module_id,
        unlock_at: row.unlock_at_us.map(from_epoch_micros).transpose()?,
        prerequisite_ids: row.prerequisite_ids.clone(),
        require_sequential: row.require_sequential,
        completion_rule: CompletionRule::parse(&row.completion_rule).ok_or(Error::Stored {
            id: row.module_id,
            column: "completion_rule",
        })?,
        items: Vec::new(),
    })
}

/// A stored requirement this release cannot read is an error, not a missing requirement, so an
/// item never opens a sequential module or completes one by a requirement being skipped.
fn stored_requirement(id: Uuid, text: &str) -> Result<Requirement, Error> {
    let stored = || Error::Stored {
        id,
        column: "requirement",
    };
    let value: serde_json::Value = serde_json::from_str(text).map_err(|_| stored())?;
    Requirement::parse(&value).map_err(|_| stored())
}

/// Why [`require_unlocked`] refused an item.
#[derive(Debug, thiserror::Error)]
pub enum RequireError {
    /// No published item with that id in a published module of the scope's organization: what
    /// a learner cannot see does not exist for them (spec 03 B13).
    #[error("the item does not exist")]
    NotFound,
    /// The item is locked for the learner, for these reasons (never empty): `item.locked`.
    #[error("the item is locked")]
    Locked(Vec<LockReason>),
    #[error(transparent)]
    Repository(#[from] Error),
}

/// Refuse unless the item `item_id` of the scope's organization is published, in a published
/// module, and unlocked at `now` for a learner who has completed the items `completed` (B13).
/// Evaluates the item's whole course with [`evaluate`], so the answer is the one the learner's
/// reads show.
///
/// The caller must already have passed the route's item target through the server's
/// `require::check_course`, which hides a course the caller cannot see, a draft and a template.
/// This function does not check the course, so without that check its answers would tell a
/// caller whether an item exists in a course they cannot see.
pub async fn require_unlocked(
    conn: &mut PgConnection,
    scope: OrgScope,
    item_id: Uuid,
    completed: &HashSet<Uuid>,
    now: SystemTime,
) -> Result<(), RequireError> {
    let course_id = items::course_of(&mut *conn, scope, item_id)
        .await?
        .ok_or(RequireError::NotFound)?;
    let rules = load_course_rules(&mut *conn, scope, course_id).await?;
    check_item(&evaluate(&rules, completed, now), item_id)
}

/// [`require_unlocked`]'s answer for the item `item_id` from an `evaluation` of its course the
/// caller already holds: [`RequireError::NotFound`] when the item is not a published item of a
/// published module, [`RequireError::Locked`] with its reasons when it is locked.
pub fn check_item(evaluation: &Evaluation, item_id: Uuid) -> Result<(), RequireError> {
    match evaluation.item(item_id).map(|item| &item.state) {
        None => Err(RequireError::NotFound),
        Some(State::Available) => Ok(()),
        Some(State::Locked(reasons)) => Err(RequireError::Locked(reasons.clone())),
    }
}

#[cfg(test)]
mod tests;
