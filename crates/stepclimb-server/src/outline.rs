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
//! can see. Until spec 04 I3 computes lock state, [`withholds_content`] keeps
//! the references and URLs of every item of a module with an unlock rule from them.
//!
//! A caller who holds `course.participate` at the course gets their own progress on each module
//! and item of the answer (spec 05 B13), read for their own user id. Progress is looked up by the
//! ids the answer already holds, so it never names a module or item the caller cannot see.

use std::collections::HashMap;

use axum::{Json, extract::State};
use sqlx::{Postgres, Transaction};
use stepclimb_core::{
    courses,
    grants::Denied,
    id::Uuid,
    items,
    modules::{self, Error},
    perm::{CourseParticipate, CourseView, CourseViewAll},
    progress,
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
    let reader = Reader::new(&mut tx, &require, viewer, course_id, &all_modules).await?;
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
    let reader = Reader::new(&mut tx, &require, viewer, course_id, &all_modules).await?;
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
    let reader = Reader::new(&mut tx, &require, viewer, course_id, &all_modules).await?;
    end(tx).await?;
    let module = all_modules
        .iter()
        .find(|module| Some(module.id) == module_id && reader.sees(module))
        .ok_or_else(|| content_error(Error::ItemNotFound))?;
    let mut visible: Vec<_> = siblings
        .into_iter()
        .filter(|item| reader.sees_item(item))
        .collect();
    let index = visible
        .iter()
        .position(|item| item.id == target.id)
        .ok_or_else(|| content_error(Error::ItemNotFound))?;
    let mut answer = reader.item(module, visible.swap_remove(index))?;
    reader.renumber(&mut answer.position, index);
    Ok(Json(answer))
}

/// Whether a learner's read of `module` withholds its items' `ref_id` and `url`: whenever the
/// module has any unlock rule (an unlock date, past or future, a prerequisite, or sequential
/// order). Items carry no unlock rule of their own; a requirement only completes an item.
///
/// A stand-in that fails closed. It does not decide whether the module or item is locked, which
/// needs the learner's progress; spec 04 I3 replaces this function with the lock state of
/// `stepclimb_core::unlock::evaluate`, which withholds them only from what is locked (spec 04
/// B12). Callers holding `course.view_all` never reach it.
fn withholds_content(module: &modules::Module) -> bool {
    module.unlock_at.is_some() || !module.prerequisite_ids.is_empty() || module.require_sequential
}

/// `item` without what it points at.
fn withhold(item: &mut Item) {
    item.ref_id = None;
    item.url = None;
}

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
}

impl Reader {
    /// The reader for `viewer`, reading a learner's own progress in `course_id` in `tx`.
    async fn new<T>(
        tx: &mut Transaction<'static, Postgres>,
        require: &Require<CourseView, T>,
        viewer: Viewer,
        course_id: Uuid,
        all_modules: &[modules::Module],
    ) -> Result<Self, ApiError> {
        let own = if viewer.learner {
            let (scope, user_id) = (require.grants().scope(), require.principal().user_id);
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
        Ok(Self::from_parts(viewer.sees_all, all_modules, own))
    }

    fn from_parts(sees_all: bool, all_modules: &[modules::Module], own: Option<Own>) -> Self {
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
    /// ([`Self::renumber`]), references and URLs withheld as [`withholds_content`] says; and a
    /// learner's own progress on the module and each item.
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
                let mut answer = self.item(&module, item)?;
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

    /// `item` of `module` as the caller may see it, with its stored position.
    fn item(&self, module: &modules::Module, item: items::Item) -> Result<Item, ApiError> {
        let mut answer = Item::from_repo(item)?;
        if !self.sees_all && withholds_content(module) {
            withhold(&mut answer);
        }
        if let Some(own) = &self.own {
            let stored = own.items.get(&answer.id).cloned();
            answer.progress = Some(stored.map(ItemProgress::from_core).transpose()?);
        }
        Ok(answer)
    }
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

    #[test]
    fn any_unlock_rule_withholds_content() {
        assert!(!withholds_content(&module(1, true)));
        let dated = modules::Module {
            unlock_at: Some(UNIX_EPOCH + Duration::from_secs(1)),
            ..module(1, true)
        };
        let gated = modules::Module {
            prerequisite_ids: vec![Uuid::from_u128(2)],
            ..module(1, true)
        };
        let sequential = modules::Module {
            require_sequential: true,
            ..module(1, true)
        };
        for ruled in [dated, gated, sequential] {
            assert!(withholds_content(&ruled), "{ruled:?}");
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
        let learner = Reader::from_parts(false, &course, None)
            .module(gated.clone(), items.clone())
            .unwrap();
        assert_eq!(learner.prerequisite_ids, [Uuid::from_u128(2)]);
        assert_eq!(learner.items.len(), 1);
        assert_eq!(learner.items[0].id, Uuid::from_u128(10));
        assert_eq!(learner.items[0].url, None, "a gated module withholds URLs");

        let staff = Reader::from_parts(true, &course, None)
            .module(gated, items)
            .unwrap();
        assert_eq!(staff.prerequisite_ids.len(), 2);
        assert_eq!(staff.items.len(), 2);
        assert!(staff.items.iter().all(|item| item.url.is_some()));
    }

    #[test]
    fn a_learner_gets_the_urls_of_a_module_without_rules() {
        let open = Reader::from_parts(false, &[module(1, true)], None)
            .module(module(1, true), vec![link(10, 1, true)])
            .unwrap();
        assert_eq!(open.items[0].url.as_deref(), Some("https://example.com/a"));
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
        let learner = Reader::from_parts(false, &[module(1, true)], Some(own))
            .module(module(1, true), items.clone())
            .unwrap();
        assert_eq!(learner.progress, Some(None), "no module row: null");
        assert!(matches!(learner.items[0].progress, Some(Some(_))));
        assert_eq!(learner.items[1].progress, Some(None), "no item row: null");

        let staff = Reader::from_parts(true, &[module(1, true)], None)
            .module(module(1, true), items)
            .unwrap();
        assert_eq!(staff.progress, None);
        assert!(staff.items.iter().all(|item| item.progress.is_none()));
    }
}
