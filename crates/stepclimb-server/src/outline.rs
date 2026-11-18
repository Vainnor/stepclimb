// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reading a course's outline (docs/specs/m1/03-modules-and-items.md B13 to B15 and API): the
//! course's modules with their items, one module, one item.
//!
//! Every route takes `Require<perm::CourseView, _>` at the course, directly or through the module
//! ([`ModuleTarget`]) or the item ([`ItemTarget`]), so a draft or a template answers 404 to a
//! caller without `course.view_all` (spec 02 B24). The handler reads in one `REPEATABLE READ`
//! transaction and checks the course again on the row it reads there, so the visibility check,
//! the outline and the caller's progress come from the same snapshot.
//!
//! A caller without `course.view_all` at the course (a learner) sees only published modules and,
//! in them, only published items; an unpublished module or item, or an item of an unpublished
//! module, answers 404 as an id that does not exist. A module's `prerequisite_ids` keep only
//! published modules for them, and every `position` is numbered over the modules or items they
//! can see.
//!
//! For the same callers, each module and item carries its `lock` (docs/specs/m1/04-unlock-and-
//! completion-rules.md B5, B12, B14): `stepclimb_core::unlock::evaluate` over the course's
//! published rules, the items the caller has completed (none for a caller who is not a learner,
//! such as an Observer, spec 05 B18) and the request clock's time, all read in the read's
//! snapshot after the course check. A locked item keeps its title, kind, position, requirement
//! and the caller's own progress, and leaves out `ref_id` and `url`. A holder of
//! `course.view_all` gets every module and item with `lock: null` and everything they point at.
//!
//! A caller who holds `course.participate` at the course gets their own progress on each module
//! and item of the answer (spec 05 B13), read for their own user id. Progress is looked up by the
//! ids the answer already holds, so it never names a module or item the caller cannot see.

use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

use axum::{Json, extract::State};
use sqlx::{Postgres, Transaction};
use stepclimb_core::{
    courses,
    grants::Denied,
    id::Uuid,
    items,
    modules::{self, Error},
    perm::{CourseParticipate, CourseView, CourseViewAll},
    progress, unlock,
};

use crate::{
    AppState,
    error::ApiError,
    modules::{Item, MODULE_NOT_FOUND, Module, ModuleCourseNotFound, ModuleList, content_error},
    openapi::problem_responses,
    progress::{ItemProgress, ModuleProgress, progress_error},
    require::{
        CourseTarget, ItemTarget, ModuleTarget, Require, check_course,
        code::{COURSE_NOT_FOUND, ITEM_NOT_FOUND},
        not_found_as,
    },
    unlock::Lock,
};

problem_responses! {
    ModuleReadNotFound => "`module.not_found`: no module with this id exists in the organization, or you cannot see it: its course is out of your reach, a draft course or a template, or the module is unpublished, and you do not hold `course.view_all` at the course. `org.not_found`: the request's organization cannot be determined.",
    ItemReadNotFound => "`item.not_found`: no item with this id exists in the organization, or you cannot see it: its course is out of your reach, a draft course or a template, or the item or its module is unpublished, and you do not hold `course.view_all` at the course. `org.not_found`: the request's organization cannot be determined.",
}

/// The course's modules in order, each with its items in order. A caller without
/// `course.view_all` gets only published modules and published items; a learner of the course
/// also gets their progress on each.
#[utoipa::path(
    get,
    path = "/api/v1/courses/{course_id}/modules",
    operation_id = "listModules",
    tag = "modules",
    params(("course_id" = String, Path, format = "uuid", description = "The course's id.")),
    responses(
        (status = 200, description = "The course's outline", body = ModuleList),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = ModuleCourseNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_modules(
    require: Require<CourseView, CourseTarget>,
    State(state): State<AppState>,
) -> Result<Json<ModuleList>, ApiError> {
    let course_id = require.target().id;
    let scope = require.grants().scope();
    let mut tx = begin_read(&state).await?;
    let viewer = view(&mut tx, &require, course_id, COURSE_NOT_FOUND).await?;
    let all_modules = modules::list_for_course(&mut *tx, scope, course_id)
        .await
        .map_err(content_error)?;
    let all_items = items::list_for_course(&mut *tx, scope, course_id)
        .await
        .map_err(content_error)?;
    let now = state.clock.now();
    let reader = Reader::new(&mut tx, &require, viewer, course_id, &all_modules, now).await?;
    end(tx).await?;
    let mut by_module: HashMap<Uuid, Vec<items::Item>> = HashMap::new();
    for item in all_items {
        by_module.entry(item.module_id).or_default().push(item);
    }
    let modules = all_modules
        .into_iter()
        .filter(|module| reader.sees(module))
        .map(|module| {
            let items = by_module.remove(&module.id).unwrap_or_default();
            reader.module(module, items)
        })
        .collect::<Result<_, _>>()?;
    Ok(Json(ModuleList { modules }))
}

/// One module with its items in order. An unpublished module answers 404 to a caller without
/// `course.view_all`, who also gets only its published items; a learner of the course also gets
/// their progress.
#[utoipa::path(
    get,
    path = "/api/v1/modules/{module_id}",
    operation_id = "getModule",
    tag = "modules",
    params(("module_id" = String, Path, format = "uuid", description = "The module's id.")),
    responses(
        (status = 200, description = "The module", body = Module),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = ModuleReadNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_module(
    require: Require<CourseView, ModuleTarget>,
    State(state): State<AppState>,
) -> Result<Json<Module>, ApiError> {
    let target = require.target();
    let (scope, course_id) = (require.grants().scope(), target.course.id);
    let mut tx = begin_read(&state).await?;
    let viewer = view(&mut tx, &require, course_id, MODULE_NOT_FOUND).await?;
    let all_modules = modules::list_for_course(&mut *tx, scope, course_id)
        .await
        .map_err(content_error)?;
    let items = items::list_for_module(&mut *tx, scope, target.id)
        .await
        .map_err(content_error)?;
    let now = state.clock.now();
    let reader = Reader::new(&mut tx, &require, viewer, course_id, &all_modules, now).await?;
    end(tx).await?;
    let module = all_modules
        .into_iter()
        .find(|module| module.id == target.id && reader.sees(module))
        .ok_or_else(|| content_error(Error::ModuleNotFound))?;
    Ok(Json(reader.module(module, items)?))
}

/// One item. An unpublished item, or an item of an unpublished module, answers 404 to a caller
/// without `course.view_all`; a learner of the course also gets their progress on it.
#[utoipa::path(
    get,
    path = "/api/v1/items/{item_id}",
    operation_id = "getItem",
    tag = "modules",
    params(("item_id" = String, Path, format = "uuid", description = "The item's id.")),
    responses(
        (status = 200, description = "The item", body = Item),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = ItemReadNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_item(
    require: Require<CourseView, ItemTarget>,
    State(state): State<AppState>,
) -> Result<Json<Item>, ApiError> {
    let target = require.target();
    let (scope, course_id) = (require.grants().scope(), target.course.id);
    let mut tx = begin_read(&state).await?;
    let viewer = view(&mut tx, &require, course_id, ITEM_NOT_FOUND).await?;
    let all_modules = modules::list_for_course(&mut *tx, scope, course_id)
        .await
        .map_err(content_error)?;
    let module_id = items::get(&mut *tx, scope, target.id)
        .await
        .map_err(content_error)?
        .map(|item| item.module_id);
    let siblings = match module_id {
        Some(module_id) => items::list_for_module(&mut *tx, scope, module_id)
            .await
            .map_err(content_error)?,
        None => Vec::new(),
    };
    let now = state.clock.now();
    let reader = Reader::new(&mut tx, &require, viewer, course_id, &all_modules, now).await?;
    end(tx).await?;
    if !all_modules
        .iter()
        .any(|module| Some(module.id) == module_id && reader.sees(module))
    {
        return Err(content_error(Error::ItemNotFound));
    }
    let mut visible: Vec<_> = siblings
        .into_iter()
        .filter(|item| reader.sees_item(item))
        .collect();
    let index = visible
        .iter()
        .position(|item| item.id == target.id)
        .ok_or_else(|| content_error(Error::ItemNotFound))?;
    let mut answer = reader.item(visible.swap_remove(index))?;
    reader.renumber(&mut answer.position, index);
    Ok(Json(answer))
}

/// `item` without what it points at.
fn withhold(item: &mut Item) {
    item.ref_id = None;
    item.url = None;
}

/// A module or item that is visible to the caller but missing from the course's evaluation,
/// which reads the same snapshot: never expected, and a server error rather than an open lock.
#[derive(Debug, thiserror::Error)]
#[error("{0} is visible but not in its course's evaluation")]
struct NotEvaluated(Uuid);

/// Who reads, as the read's snapshot decides it.
#[derive(Clone, Copy, Debug)]
struct Viewer {
    /// Holds `course.view_all` at the course: sees unpublished content and every reference.
    sees_all: bool,
    /// Holds `course.participate` at the course: gets their own progress (spec 05 B13).
    learner: bool,
}

/// The caller's own progress rows, by module and item id.
#[derive(Debug, Default)]
struct Own {
    modules: HashMap<Uuid, progress::ModuleProgress>,
    items: HashMap<Uuid, progress::ItemProgress>,
}

/// How one read presents rows to its caller.
#[derive(Debug)]
struct Reader {
    sees_all: bool,
    /// The course's published modules, each with its position among them.
    published: HashMap<Uuid, usize>,
    /// `Some` for a learner of the course, `None` for anyone else.
    own: Option<Own>,
    /// The caller's lock state in the course; `None` for a holder of `course.view_all`, who is
    /// not held to it (spec 04 B5).
    locks: Option<unlock::Evaluation>,
}

impl Reader {
    /// The reader for `viewer`, reading a learner's own progress in `course_id` in `tx` and,
    /// for a caller without `course.view_all`, their lock state at `now`.
    async fn new<T>(
        tx: &mut Transaction<'static, Postgres>,
        require: &Require<CourseView, T>,
        viewer: Viewer,
        course_id: Uuid,
        all_modules: &[modules::Module],
        now: SystemTime,
    ) -> Result<Self, ApiError> {
        let scope = require.grants().scope();
        let own = if viewer.learner {
            let user_id = require.principal().user_id;
            let stored = progress::for_learner_in_course(tx, scope, user_id, course_id)
                .await
                .map_err(progress_error)?;
            Some(Own {
                modules: stored
                    .modules
                    .into_iter()
                    .map(|p| (p.module_id, p))
                    .collect(),
                items: stored.items.into_iter().map(|p| (p.item_id, p)).collect(),
            })
        } else {
            None
        };
        let locks = if viewer.sees_all {
            None
        } else {
            let rules = unlock::load_course_rules(&mut **tx, scope, course_id)
                .await
                .map_err(content_error)?;
            Some(unlock::evaluate(&rules, &completed(own.as_ref()), now))
        };
        Ok(Self::from_parts(viewer.sees_all, all_modules, own, locks))
    }

    fn from_parts(
        sees_all: bool,
        all_modules: &[modules::Module],
        own: Option<Own>,
        locks: Option<unlock::Evaluation>,
    ) -> Self {
        let published = all_modules
            .iter()
            .filter(|module| module.published)
            .enumerate()
            .map(|(index, module)| (module.id, index))
            .collect();
        Self {
            sees_all,
            published,
            own,
            locks,
        }
    }

    fn sees(&self, module: &modules::Module) -> bool {
        self.sees_all || module.published
    }

    fn sees_item(&self, item: &items::Item) -> bool {
        self.sees_all || item.published
    }

    /// `module` with `items` as the caller may see them: for a caller without `course.view_all`,
    /// only published items and published prerequisites, positions numbered over what they see
    /// ([`Self::renumber`]), and the module's and each item's `lock`, a locked item without its
    /// reference and URL; and a learner's own progress on the module and each item.
    fn module(
        &self,
        mut module: modules::Module,
        items: Vec<items::Item>,
    ) -> Result<Module, ApiError> {
        if !self.sees_all {
            module
                .prerequisite_ids
                .retain(|id| self.published.contains_key(id));
        }
        let items = items
            .into_iter()
            .filter(|item| self.sees_item(item))
            .enumerate()
            .map(|(index, item)| {
                let mut answer = self.item(item)?;
                self.renumber(&mut answer.position, index);
                Ok(answer)
            })
            .collect::<Result<_, ApiError>>()?;
        let index = self.published.get(&module.id).copied();
        let mut answer = Module::from_repo(module, Vec::new())?;
        answer.items = items;
        if let Some(index) = index {
            self.renumber(&mut answer.position, index);
        }
        if let Some(locks) = &self.locks {
            let state = locks.module(answer.id).map(|module| &module.state);
            answer.lock = lock(state, answer.id)?;
        }
        if let Some(own) = &self.own {
            let stored = own.modules.get(&answer.id).cloned();
            answer.progress = Some(stored.map(ModuleProgress::from_core).transpose()?);
        }
        Ok(answer)
    }

    /// For a caller without `course.view_all`, `position` becomes `index`, the row's place among
    /// the rows they can see, so the numbering does not reveal how many unpublished rows sit
    /// before it. Staff keep the stored position, which is the same numbering over every row.
    fn renumber(&self, position: &mut i32, index: usize) {
        if !self.sees_all {
            // An index is under 200, the most modules a course or items a module holds.
            *position = i32::try_from(index).unwrap_or(i32::MAX);
        }
    }

    /// `item` as the caller may see it, with its stored position: for a caller held to the
    /// lock, its `lock`, and without its reference and URL while it is locked.
    fn item(&self, item: items::Item) -> Result<Item, ApiError> {
        let mut answer = Item::from_repo(item)?;
        if let Some(locks) = &self.locks {
            let state = locks.item(answer.id).map(|item| &item.state);
            answer.lock = lock(state, answer.id)?;
            if answer.lock.is_some() {
                withhold(&mut answer);
            }
        }
        if let Some(own) = &self.own {
            let stored = own.items.get(&answer.id).cloned();
            answer.progress = Some(stored.map(ItemProgress::from_core).transpose()?);
        }
        Ok(answer)
    }
}

/// The `lock` member for the evaluated `state` of the module or item `id`. A row the caller can
/// see is always in the evaluation, which reads the same snapshot; one that is not is a server
/// error, never an open lock.
fn lock(state: Option<&unlock::State>, id: Uuid) -> Result<Option<Lock>, ApiError> {
    let state = state.ok_or_else(|| ApiError::internal(&NotEvaluated(id)))?;
    Lock::from_state(state)
}

/// The ids of the items the learner `own` has completed; none for a caller who is not a
/// learner.
fn completed(own: Option<&Own>) -> HashSet<Uuid> {
    own.into_iter()
        .flat_map(|own| own.items.values())
        .filter(|item| item.state == progress::State::Completed)
        .map(|item| item.item_id)
        .collect()
}

/// `require`'s check of `course.view` again, on the course as the read's snapshot holds it: 404
/// with `not_found` when the course is gone, out of reach, or a draft or a template the caller
/// may not see. Returns whether the caller holds `course.view_all` at the course, which decides
/// whether unpublished modules and items are shown, and `course.participate`, which decides
/// whether their progress is.
async fn view<T>(
    tx: &mut Transaction<'static, Postgres>,
    require: &Require<CourseView, T>,
    course_id: Uuid,
    not_found: &'static str,
) -> Result<Viewer, ApiError> {
    let grants = require.grants();
    let located = courses::locate(&mut **tx, grants.scope(), course_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(|| not_found_as(Denied::CourseNotFound, not_found))?;
    let course = CourseTarget {
        id: course_id,
        account_path: located.account_path,
    };
    check_course::<CourseView>(grants, &course, located.needs_view_all, not_found)?;
    let target = course.target();
    Ok(Viewer {
        sees_all: grants.holds::<CourseViewAll>(&target),
        learner: grants.holds::<CourseParticipate>(&target),
    })
}

/// A read-only transaction whose statements all see one snapshot.
async fn begin_read(state: &AppState) -> Result<Transaction<'static, Postgres>, ApiError> {
    state
        .db
        .begin_with("BEGIN ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .await
        .map_err(|error| ApiError::internal(&error))
}

async fn end(tx: Transaction<'static, Postgres>) -> Result<(), ApiError> {
    tx.commit()
        .await
        .map_err(|error| ApiError::internal(&error))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use stepclimb_core::{items::Kind, rules::CompletionRule};

    use super::*;
    use crate::modules::{ItemKind, Requirement};

    fn module(id: u128, published: bool) -> modules::Module {
        modules::Module {
            id: Uuid::from_u128(id),
            course_id: Uuid::from_u128(100),
            title: "Module".into(),
            position: 0,
            published,
            unlock_at: None,
            prerequisite_ids: Vec::new(),
            require_sequential: false,
            completion_rule: CompletionRule::All,
            created_at: UNIX_EPOCH,
            updated_at: UNIX_EPOCH,
        }
    }

    fn link(id: u128, module_id: u128, published: bool) -> items::Item {
        items::Item {
            id: Uuid::from_u128(id),
            module_id: Uuid::from_u128(module_id),
            course_id: Uuid::from_u128(100),
            kind: Kind::ExternalLink,
            title: "Link".into(),
            position: 0,
            published,
            ref_id: None,
            url: Some("https://example.com/a".into()),
            requirement: None,
            created_at: UNIX_EPOCH,
            updated_at: UNIX_EPOCH,
        }
    }

    /// `rules` evaluated for a learner who completed `completed`, at `UNIX_EPOCH` + 100 s.
    fn locks(rules: &[unlock::ModuleRules], completed: &[u128]) -> Option<unlock::Evaluation> {
        let rules = unlock::CourseRules {
            modules: rules.to_vec(),
        };
        let completed = completed.iter().map(|&id| Uuid::from_u128(id)).collect();
        Some(unlock::evaluate(
            &rules,
            &completed,
            UNIX_EPOCH + Duration::from_secs(100),
        ))
    }

    fn rules(id: u128, items: &[u128]) -> unlock::ModuleRules {
        unlock::ModuleRules {
            id: Uuid::from_u128(id),
            unlock_at: None,
            prerequisite_ids: Vec::new(),
            require_sequential: false,
            completion_rule: CompletionRule::All,
            items: items
                .iter()
                .map(|&id| unlock::ItemRules {
                    id: Uuid::from_u128(id),
                    requirement: None,
                })
                .collect(),
        }
    }

    #[test]
    fn a_learner_gets_published_items_and_published_prerequisites() {
        let gated = modules::Module {
            prerequisite_ids: vec![Uuid::from_u128(2), Uuid::from_u128(3)],
            ..module(1, true)
        };
        let course = [gated.clone(), module(2, true), module(3, false)];
        let items = vec![link(10, 1, true), link(11, 1, false)];
        // Module 2 has no requirement and is open, so it satisfies module 1's prerequisite.
        let evaluated = rules(1, &[10]);
        let evaluated = unlock::ModuleRules {
            prerequisite_ids: vec![Uuid::from_u128(2)],
            ..evaluated
        };
        let learner = Reader::from_parts(
            false,
            &course,
            None,
            locks(&[evaluated, rules(2, &[])], &[]),
        )
        .module(gated.clone(), items.clone())
        .unwrap();
        assert_eq!(learner.prerequisite_ids, [Uuid::from_u128(2)]);
        assert_eq!(learner.lock, None);
        assert_eq!(learner.items.len(), 1);
        assert_eq!(learner.items[0].id, Uuid::from_u128(10));
        assert_eq!(
            learner.items[0].url.as_deref(),
            Some("https://example.com/a"),
            "a module whose prerequisites are met shows its links"
        );

        let staff = Reader::from_parts(true, &course, None, None)
            .module(gated, items)
            .unwrap();
        assert_eq!(staff.prerequisite_ids.len(), 2);
        assert_eq!(staff.items.len(), 2);
        assert!(staff.items.iter().all(|item| item.url.is_some()));
        assert!(staff.items.iter().all(|item| item.lock.is_none()));
    }

    #[test]
    fn a_locked_item_shows_its_reasons_and_leaves_out_its_link() {
        let later = UNIX_EPOCH + Duration::from_secs(200);
        let dated = modules::Module {
            unlock_at: Some(later),
            ..module(1, true)
        };
        let evaluated = unlock::ModuleRules {
            unlock_at: Some(later),
            ..rules(1, &[10])
        };
        let mut item = link(10, 1, true);
        item.requirement = Some(stepclimb_core::rules::Requirement::View);
        let learner = Reader::from_parts(
            false,
            std::slice::from_ref(&dated),
            None,
            locks(&[evaluated], &[]),
        )
        .module(dated, vec![item])
        .unwrap();
        let reasons = |lock: &Option<Lock>| lock.as_ref().map(|lock| lock.reasons.clone());
        assert_eq!(
            reasons(&learner.lock),
            Some(vec![crate::unlock::LockReason::UnlockAt {
                at: "1970-01-01T00:03:20.000000Z".into()
            }])
        );
        let item = &learner.items[0];
        assert_eq!(
            reasons(&item.lock),
            Some(vec![crate::unlock::LockReason::ModuleLocked])
        );
        assert_eq!((&item.url, &item.ref_id), (&None, &None));
        assert_eq!(
            (item.title.as_str(), item.kind, item.requirement),
            ("Link", ItemKind::ExternalLink, Some(Requirement::View)),
            "the title, kind and requirement stay"
        );
    }

    #[test]
    fn a_learner_gets_the_urls_of_an_open_module() {
        let open = Reader::from_parts(
            false,
            &[module(1, true)],
            None,
            locks(&[rules(1, &[10])], &[]),
        )
        .module(module(1, true), vec![link(10, 1, true)])
        .unwrap();
        assert_eq!(open.lock, None);
        assert_eq!(open.items[0].lock, None);
        assert_eq!(open.items[0].url.as_deref(), Some("https://example.com/a"));
    }

    #[test]
    fn a_visible_row_missing_from_the_evaluation_is_a_server_error() {
        let reader = Reader::from_parts(false, &[module(1, true)], None, locks(&[], &[]));
        let error = reader
            .module(module(1, true), Vec::new())
            .expect_err("not evaluated");
        assert_eq!(
            error.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
        let reader = Reader::from_parts(
            false,
            &[module(1, true)],
            None,
            locks(&[rules(1, &[])], &[]),
        );
        let error = reader
            .module(module(1, true), vec![link(10, 1, true)])
            .expect_err("the item is not evaluated");
        assert_eq!(
            error.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn only_completed_rows_count_as_completed() {
        let row = |id: u128, state| progress::ItemProgress {
            item_id: Uuid::from_u128(id),
            state,
            started_at: UNIX_EPOCH,
            completed_at: (state == progress::State::Completed).then_some(UNIX_EPOCH),
            updated_at: UNIX_EPOCH,
        };
        let own = Own {
            modules: HashMap::new(),
            items: [
                row(1, progress::State::Completed),
                row(2, progress::State::Started),
            ]
            .into_iter()
            .map(|p| (p.item_id, p))
            .collect(),
        };
        assert_eq!(completed(Some(&own)), HashSet::from([Uuid::from_u128(1)]));
        assert!(completed(None).is_empty());
    }

    #[test]
    fn progress_is_present_for_a_learner_and_absent_otherwise() {
        let started = progress::ItemProgress {
            item_id: Uuid::from_u128(10),
            state: progress::State::Started,
            started_at: UNIX_EPOCH,
            completed_at: None,
            updated_at: UNIX_EPOCH,
        };
        let own = Own {
            modules: HashMap::new(),
            items: HashMap::from([(started.item_id, started)]),
        };
        let items = vec![link(10, 1, true), link(11, 1, true)];
        let learner = Reader::from_parts(
            false,
            &[module(1, true)],
            Some(own),
            locks(&[rules(1, &[10, 11])], &[]),
        )
        .module(module(1, true), items.clone())
        .unwrap();
        assert_eq!(learner.progress, Some(None), "no module row: null");
        assert!(matches!(learner.items[0].progress, Some(Some(_))));
        assert_eq!(learner.items[1].progress, Some(None), "no item row: null");

        let staff = Reader::from_parts(true, &[module(1, true)], None, None)
            .module(module(1, true), items)
            .unwrap();
        assert_eq!(staff.progress, None);
        assert!(staff.items.iter().all(|item| item.progress.is_none()));
    }
}
