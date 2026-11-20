// SPDX-License-Identifier: AGPL-3.0-or-later
//! Authoring a module's items (docs/specs/m1/03-modules-and-items.md B4 to B12 and API):
//! create, rename, publish, change an external link's URL, reorder, move between the course's
//! modules, and delete.
//!
//! Every route takes `Require<perm::CourseContentEdit, _>` at the item's course, through the
//! module ([`ModuleTarget`]) or the item ([`ItemTarget`]), and checks it again under the course
//! lock as [`crate::modules`] does. A move's destination module must be in the same course, so
//! the one check covers both ends.
//!
//! A write that can change learners' counts enqueues `progress.recompute_course` for the course
//! in its transaction (docs/specs/m1/05-progress-and-events.md B8): publishing or unpublishing
//! an item, changing its requirement, moving it to another module, and deleting it. Creating
//! (unpublished), renaming, a link's URL and reordering within a module change no count.

use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use sqlx::{Postgres, Transaction};
use stepclimb_core::{OrgScope, id::Uuid, items, perm::CourseContentEdit};
use utoipa::ToSchema;

use crate::{
    AppState,
    courses::nullable,
    error::ApiError,
    extract::ApiJson,
    modules::{
        ContentEditForbidden, Item, ItemKind, MODULE_NOT_FOUND, Module, ModuleNotFound,
        Requirement, begin, commit, content_error, recheck,
    },
    openapi::problem_responses,
    organization::present,
    progress_recompute,
    require::{ItemTarget, ModuleTarget, Require, code::ITEM_NOT_FOUND},
};

/// The path of an item; `{ITEMS_PATH}/{item_id}`.
pub const ITEMS_PATH: &str = "/api/v1/items";

problem_responses! {
    ItemNotFound => "`item.not_found`: no item with this id exists in the organization, or you cannot see its course; a draft course or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined.",
    ItemMoveNotFound => "`item.not_found`: no item with this id exists in the organization, or you cannot see its course; a draft course or a template also needs `course.view_all`. `module.not_found`: the destination module is not in the item's course. `org.not_found`: the request's organization cannot be determined.",
    ItemCreateNotFound => "`module.not_found`: no module with this id exists in the organization, or you cannot see its course; a draft course or a template also needs `course.view_all`. `file.not_found`: a `file` item's `ref_id` is not a course file of the module's course. `org.not_found`: the request's organization cannot be determined.",
    ItemCreateConflict => "`course.archived`: the course is archived and takes no content change. `item.limit_reached`: the module holds 200 items.",
    ItemOrderConflict => "`course.archived`: the course is archived and takes no content change. `order.stale`: the list is not exactly the current set of ids (one is missing, extra or repeated); reload and try again.",
    ItemWriteConflict => "`course.archived`: the course is archived and takes no content change.",
    ItemDeleteConflict => "`course.archived`: the course is archived and takes no content change. `item.has_progress`: learners have progress on the item; unpublish it instead. `item.assigned`: an assignment, ended or not, names the item.",
    ItemMoveConflict => "`course.archived`: the course is archived and takes no content change. `item.limit_reached`: the destination module holds 200 items.",
    ItemInvalid => "`request.invalid`: `errors` names `/title` (`name.empty`, `name.too_long`, `text.contains_nul`), `/kind` with `item.kind_unavailable` (this release cannot create items of the kind yet), `/url` with `item.url_invalid` (not an absolute `https` or `http` URL of at most 2,048 characters with a host, without a user name, password, white space, control or Unicode format character), `item.url_required` (an external link without a URL) or `item.url_unavailable` (a URL on another kind), `/ref_id` with `item.ref_id_unavailable` (a reference on a kind that has none) or `item.ref_id_required` (a `file` item without its course file), `/position` with `item.position_out_of_range` (outside 0 to the module's item count), or, on a patch, `/requirement` with `requirement.invalid` (an unknown shape or member, `min_percent` outside 1 to 100, or a requirement the item's kind does not take, such as any on a header) or `requirement.unavailable` (`submit`, `score` or `mastery`, reserved for quizzes and activities).",
}

/// An item to create, unpublished.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewItem {
    /// `header`, `external_link` and `file` can be created; the other kinds arrive with their
    /// specs.
    pub kind: ItemKind,
    /// Trimmed; 1 to 200 characters.
    pub title: String,
    /// What the item points at, for a kind that points at a row: for `file`, required, a course
    /// file of the module's course.
    #[serde(default)]
    #[schema(value_type = Option<String>, format = "uuid")]
    pub ref_id: Option<Uuid>,
    /// For `external_link` only: an absolute `https` or `http` URL of at most 2,048 characters.
    #[serde(default)]
    pub url: Option<String>,
    /// 0 to the module's item count; last when omitted.
    #[serde(default)]
    pub position: Option<i64>,
}

/// A change to an item; an omitted member is left as it is. The kind and `ref_id` never change.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemPatch {
    /// Trimmed; 1 to 200 characters.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub title: Option<String>,
    /// Whether learners see the item.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = bool)]
    pub published: Option<bool>,
    /// For an external link only.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub url: Option<String>,
    /// What completes the item for a learner; `null` removes it. A header takes none.
    //
    // Read as a JSON object and checked by `items::parse_requirement`, so a shape the schema
    // lacks answers 422 `requirement.invalid` or `requirement.unavailable`; anything but an
    // object is 400, as for every other object of a body (`extract::ApiJson`).
    #[serde(default, deserialize_with = "nullable")]
    #[schema(value_type = Option<Requirement>)]
    pub requirement: Option<Option<serde_json::Map<String, serde_json::Value>>>,
}

/// Every item id of the module, in the new order.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemOrder {
    #[schema(value_type = Vec<String>)]
    pub item_ids: Vec<Uuid>,
}

/// Where to move an item.
#[derive(Clone, Copy, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemMove {
    /// A module of the item's course, its own included.
    #[schema(value_type = String, format = "uuid")]
    pub module_id: Uuid,
    /// 0 to the destination's item count, not counting the moved item.
    pub position: i64,
}

/// Create an unpublished item in the module, at `position` or last.
#[utoipa::path(
    post,
    path = "/api/v1/modules/{module_id}/items",
    operation_id = "createItem",
    tag = "modules",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("module_id" = String, Path, format = "uuid", description = "The module's id.")),
    request_body = NewItem,
    responses(
        (
            status = 201,
            description = "The item is created",
            body = Item,
            headers(("Location" = String, description = "The item's path"))
        ),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ContentEditForbidden),
        (status = 404, response = ItemCreateNotFound),
        (status = 409, response = ItemCreateConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = ItemInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn create_item(
    require: Require<CourseContentEdit, ModuleTarget>,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<NewItem>,
) -> Result<Response, ApiError> {
    let (grants, target) = (require.grants(), require.target());
    let mut tx = begin(&state).await?;
    recheck(&mut tx, &require, target.course.id, MODULE_NOT_FOUND).await?;
    let new = items::NewItem {
        kind: body.kind.into(),
        title: body.title,
        ref_id: body.ref_id,
        url: body.url,
        position: body.position,
    };
    let item = items::create(&mut *tx, grants.scope(), target.id, &new, state.clock.now())
        .await
        .map_err(content_error)?;
    commit(tx).await?;
    let location = format!("{ITEMS_PATH}/{}", item.id);
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(Item::from_repo(item)?),
    )
        .into_response())
}

/// Put the module's items in a new order. The list must be exactly the module's current item
/// ids; the current order changes nothing.
#[utoipa::path(
    put,
    path = "/api/v1/modules/{module_id}/items/order",
    operation_id = "reorderItems",
    tag = "modules",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("module_id" = String, Path, format = "uuid", description = "The module's id.")),
    request_body = ItemOrder,
    responses(
        (status = 200, description = "The module with its items in their new order", body = Module),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ContentEditForbidden),
        (status = 404, response = ModuleNotFound),
        (status = 409, response = ItemOrderConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn reorder_items(
    require: Require<CourseContentEdit, ModuleTarget>,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<ItemOrder>,
) -> Result<Json<Module>, ApiError> {
    let (grants, target) = (require.grants(), require.target());
    let scope = grants.scope();
    let mut tx = begin(&state).await?;
    recheck(&mut tx, &require, target.course.id, MODULE_NOT_FOUND).await?;
    let ordered = items::reorder(&mut *tx, scope, target.id, &body.item_ids)
        .await
        .map_err(content_error)?;
    let module = stepclimb_core::modules::get(&mut *tx, scope, target.id)
        .await
        .map_err(content_error)?
        .ok_or_else(|| content_error(stepclimb_core::modules::Error::ModuleNotFound))?;
    commit(tx).await?;
    Ok(Json(Module::from_repo(module, ordered)?))
}

/// Rename, publish or unpublish an item, change an external link's URL, or set or remove its
/// requirement. A patch naming only current values changes nothing.
#[utoipa::path(
    patch,
    path = "/api/v1/items/{item_id}",
    operation_id = "updateItem",
    tag = "modules",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("item_id" = String, Path, format = "uuid", description = "The item's id.")),
    request_body = ItemPatch,
    responses(
        (status = 200, description = "The item after the change", body = Item),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ContentEditForbidden),
        (status = 404, response = ItemNotFound),
        (status = 409, response = ItemWriteConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = ItemInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_item(
    require: Require<CourseContentEdit, ItemTarget>,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<ItemPatch>,
) -> Result<Json<Item>, ApiError> {
    let (grants, target) = (require.grants(), require.target());
    let requirement = body
        .requirement
        .as_ref()
        .map(|r| {
            r.as_ref()
                .map(|object| items::parse_requirement(&serde_json::Value::Object(object.clone())))
                .transpose()
        })
        .transpose()
        .map_err(content_error)?;
    let mut tx = begin(&state).await?;
    recheck(&mut tx, &require, target.course.id, ITEM_NOT_FOUND).await?;
    let patch = items::ItemPatch {
        title: body.title,
        published: body.published,
        url: body.url,
    };
    let (scope, now) = (grants.scope(), state.clock.now());
    let before = existing(&mut tx, scope, target.id).await?;
    let mut item = items::update(&mut *tx, scope, target.id, &patch, now)
        .await
        .map_err(content_error)?;
    if let Some(requirement) = requirement {
        item = items::set_requirement(&mut *tx, scope, target.id, requirement, now)
            .await
            .map_err(content_error)?;
    }
    if before.published != item.published || before.requirement != item.requirement {
        progress_recompute::enqueue(&mut tx, scope, target.course.id, now).await?;
    }
    commit(tx).await?;
    Ok(Json(Item::from_repo(item)?))
}

/// Move an item to a position in a module of the same course, its own included.
#[utoipa::path(
    post,
    path = "/api/v1/items/{item_id}/move",
    operation_id = "moveItem",
    tag = "modules",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("item_id" = String, Path, format = "uuid", description = "The item's id.")),
    request_body = ItemMove,
    responses(
        (status = 200, description = "The item after the move", body = Item),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ContentEditForbidden),
        (status = 404, response = ItemMoveNotFound),
        (status = 409, response = ItemMoveConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = ItemInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn move_item(
    require: Require<CourseContentEdit, ItemTarget>,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<ItemMove>,
) -> Result<Json<Item>, ApiError> {
    let (grants, target) = (require.grants(), require.target());
    let mut tx = begin(&state).await?;
    recheck(&mut tx, &require, target.course.id, ITEM_NOT_FOUND).await?;
    let (scope, now) = (grants.scope(), state.clock.now());
    let before = existing(&mut tx, scope, target.id).await?;
    let item = items::move_to(
        &mut *tx,
        scope,
        target.id,
        body.module_id,
        body.position,
        now,
    )
    .await
    .map_err(content_error)?;
    if before.module_id != item.module_id {
        progress_recompute::enqueue(&mut tx, scope, target.course.id, now).await?;
    }
    commit(tx).await?;
    Ok(Json(Item::from_repo(item)?))
}

/// Delete an item. What it points at stays in the course. An item any learner has progress on
/// cannot be deleted; unpublish it instead.
#[utoipa::path(
    delete,
    path = "/api/v1/items/{item_id}",
    operation_id = "deleteItem",
    tag = "modules",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("item_id" = String, Path, format = "uuid", description = "The item's id.")),
    responses(
        (status = 204, description = "The item is deleted"),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ContentEditForbidden),
        (status = 404, response = ItemNotFound),
        (status = 409, response = ItemDeleteConflict),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn delete_item(
    require: Require<CourseContentEdit, ItemTarget>,
    State(state): State<AppState>,
) -> Result<StatusCode, ApiError> {
    let (grants, target) = (require.grants(), require.target());
    let mut tx = begin(&state).await?;
    recheck(&mut tx, &require, target.course.id, ITEM_NOT_FOUND).await?;
    let (scope, now) = (grants.scope(), state.clock.now());
    items::delete(&mut *tx, scope, target.id)
        .await
        .map_err(content_error)?;
    progress_recompute::enqueue(&mut tx, scope, target.course.id, now).await?;
    commit(tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The item `id` as the transaction holding its course's lock reads it.
async fn existing(
    tx: &mut Transaction<'static, Postgres>,
    scope: OrgScope,
    id: Uuid,
) -> Result<items::Item, ApiError> {
    items::get(&mut **tx, scope, id)
        .await
        .map_err(content_error)?
        .ok_or_else(|| content_error(stepclimb_core::modules::Error::ItemNotFound))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn bodies_refuse_null_and_unknown_members() {
        for refused in [
            json!({ "title": null }),
            json!({ "url": null }),
            json!({ "published": null }),
            json!({ "kind": "page" }),
            json!({ "ref_id": null }),
        ] {
            assert!(
                serde_json::from_value::<ItemPatch>(refused.clone()).is_err(),
                "{refused}"
            );
        }
        let unknown_kind = json!({ "kind": "lti", "title": "X" });
        assert!(serde_json::from_value::<NewItem>(unknown_kind).is_err());
        let header = json!({ "kind": "header", "title": "X" });
        let header: NewItem = serde_json::from_value(header).unwrap();
        assert_eq!(header.kind, ItemKind::Header);
        assert_eq!(
            (header.url, header.ref_id, header.position),
            (None, None, None)
        );
    }
}
