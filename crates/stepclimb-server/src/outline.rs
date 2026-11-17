// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reading a course's outline (docs/specs/m1/03-modules-and-items.md B13 to B15 and API): the
//! course's modules with their items, one module, one item.
//!
//! Every route takes `Require<perm::CourseView, _>` at the course, directly or through the module
//! ([`ModuleTarget`]) or the item ([`ItemTarget`]), so a draft or a template answers 404 to a
//! caller without `course.view_all` (spec 02 B24). The handler reads in one `REPEATABLE READ`
//! transaction and checks the course again on the row it reads there, so the visibility check
//! and the outline come from the same snapshot.
//!
//! A caller without `course.view_all` at the course (a learner) sees only published modules and,
//! in them, only published items; an unpublished module or item, or an item of an unpublished
//! module, answers 404 as an id that does not exist. A module's `prerequisite_ids` keep only
//! published modules for them. Until spec 04 I3 computes lock state, [`withholds_content`] keeps
//! the references and URLs of every item of a module with an unlock rule from them.

use std::collections::{HashMap, HashSet};

use axum::{Json, extract::State};
use sqlx::{Postgres, Transaction};
use stepclimb_core::{
    courses,
    grants::Denied,
    id::Uuid,
    items,
    modules::{self, Error},
    perm::{CourseView, CourseViewAll},
};

use crate::{
    AppState,
    error::ApiError,
    modules::{Item, MODULE_NOT_FOUND, Module, ModuleCourseNotFound, ModuleList, content_error},
    openapi::problem_responses,
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
/// `course.view_all` gets only published modules and published items.
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
    let mut tx = begin_read(&state).await?;
    let sees_all = view(&mut tx, &require, course_id, COURSE_NOT_FOUND).await?;
    let scope = require.grants().scope();
    let all_modules = modules::list_for_course(&mut *tx, scope, course_id)
        .await
        .map_err(content_error)?;
    let all_items = items::list_for_course(&mut *tx, scope, course_id)
        .await
        .map_err(content_error)?;
    end(tx).await?;
    let mut by_module: HashMap<Uuid, Vec<items::Item>> = HashMap::new();
    for item in all_items {
        by_module.entry(item.module_id).or_default().push(item);
    }
    let published = published_ids(&all_modules);
    let modules = all_modules
        .into_iter()
        .filter(|module| sees_all || module.published)
        .map(|module| {
            let items = by_module.remove(&module.id).unwrap_or_default();
            present_module(module, items, sees_all, &published)
        })
        .collect::<Result<_, _>>()?;
    Ok(Json(ModuleList { modules }))
}

/// One module with its items in order. An unpublished module answers 404 to a caller without
/// `course.view_all`, who also gets only its published items.
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
    let mut tx = begin_read(&state).await?;
    let sees_all = view(&mut tx, &require, target.course.id, MODULE_NOT_FOUND).await?;
    let scope = require.grants().scope();
    let all_modules = modules::list_for_course(&mut *tx, scope, target.course.id)
        .await
        .map_err(content_error)?;
    let items = items::list_for_module(&mut *tx, scope, target.id)
        .await
        .map_err(content_error)?;
    end(tx).await?;
    let published = published_ids(&all_modules);
    let module = all_modules
        .into_iter()
        .find(|module| module.id == target.id && (sees_all || module.published))
        .ok_or_else(|| content_error(Error::ModuleNotFound))?;
    Ok(Json(present_module(module, items, sees_all, &published)?))
}

/// One item. An unpublished item, or an item of an unpublished module, answers 404 to a caller
/// without `course.view_all`.
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
    let mut tx = begin_read(&state).await?;
    let sees_all = view(&mut tx, &require, target.course.id, ITEM_NOT_FOUND).await?;
    let scope = require.grants().scope();
    let item = items::get(&mut *tx, scope, target.id)
        .await
        .map_err(content_error)?;
    let module = match &item {
        Some(item) => modules::get(&mut *tx, scope, item.module_id)
            .await
            .map_err(content_error)?,
        None => None,
    };
    end(tx).await?;
    let (Some(item), Some(module)) = (item, module) else {
        return Err(content_error(Error::ItemNotFound));
    };
    if !sees_all && !(module.published && item.published) {
        return Err(content_error(Error::ItemNotFound));
    }
    let mut item = Item::from_repo(item)?;
    if !sees_all && withholds_content(&module) {
        withhold(&mut item);
    }
    Ok(Json(item))
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

/// `module` with `items` as the caller may see them: for a caller without `course.view_all`,
/// only published items, only published prerequisites, and the items' references and URLs
/// withheld as [`withholds_content`] says.
fn present_module(
    mut module: modules::Module,
    items: Vec<items::Item>,
    sees_all: bool,
    published: &HashSet<Uuid>,
) -> Result<Module, ApiError> {
    if sees_all {
        return Module::from_repo(module, items);
    }
    let withheld = withholds_content(&module);
    module.prerequisite_ids.retain(|id| published.contains(id));
    let items = items.into_iter().filter(|item| item.published).collect();
    let mut module = Module::from_repo(module, items)?;
    if withheld {
        module.items.iter_mut().for_each(withhold);
    }
    Ok(module)
}

/// The ids of the published modules among `modules`.
fn published_ids(modules: &[modules::Module]) -> HashSet<Uuid> {
    modules
        .iter()
        .filter(|module| module.published)
        .map(|module| module.id)
        .collect()
}

/// `require`'s check of `course.view` again, on the course as the read's snapshot holds it: 404
/// with `not_found` when the course is gone, out of reach, or a draft or a template the caller
/// may not see. Returns whether the caller holds `course.view_all` at the course, which decides
/// whether unpublished modules and items are shown.
async fn view<T>(
    tx: &mut Transaction<'static, Postgres>,
    require: &Require<CourseView, T>,
    course_id: Uuid,
    not_found: &'static str,
) -> Result<bool, ApiError> {
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
    Ok(grants.holds::<CourseViewAll>(&course.target()))
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
        let published = HashSet::from([Uuid::from_u128(2)]);
        let gated = modules::Module {
            prerequisite_ids: vec![Uuid::from_u128(2), Uuid::from_u128(3)],
            ..module(1, true)
        };
        let items = vec![link(10, 1, true), link(11, 1, false)];
        let learner = present_module(gated.clone(), items.clone(), false, &published).unwrap();
        assert_eq!(learner.prerequisite_ids, [Uuid::from_u128(2)]);
        assert_eq!(learner.items.len(), 1);
        assert_eq!(learner.items[0].id, Uuid::from_u128(10));
        assert_eq!(learner.items[0].url, None, "a gated module withholds URLs");

        let staff = present_module(gated, items, true, &published).unwrap();
        assert_eq!(staff.prerequisite_ids.len(), 2);
        assert_eq!(staff.items.len(), 2);
        assert!(staff.items.iter().all(|item| item.url.is_some()));
    }

    #[test]
    fn a_learner_gets_the_urls_of_a_module_without_rules() {
        let open = present_module(
            module(1, true),
            vec![link(10, 1, true)],
            false,
            &HashSet::new(),
        );
        assert_eq!(
            open.unwrap().items[0].url.as_deref(),
            Some("https://example.com/a")
        );
    }
}
