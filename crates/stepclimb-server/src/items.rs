// SPDX-License-Identifier: AGPL-3.0-or-later
//! Authoring a module's items (docs/specs/m1/03-modules-and-items.md B4 to B12 and API):
//! create, rename, publish, change an external link's URL, reorder, move between the course's
//! modules, and delete.
//!
//! Every route takes `Require<perm::CourseContentEdit, _>` at the item's course, through the
//! module ([`ModuleTarget`]) or the item ([`ItemTarget`]), and checks it again under the course
//! lock as [`crate::modules`] does. A move's destination module must be in the same course, so
//! the one check covers both ends.

use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use stepclimb_core::{id::Uuid, items, perm::CourseContentEdit};
use url::Url;
use utoipa::ToSchema;

use crate::{
    AppState,
    error::{ApiError, FieldError},
    extract::ApiJson,
    modules::{
        ContentEditForbidden, Item, ItemKind, MODULE_NOT_FOUND, Module, ModuleNotFound, begin,
        code, commit, content_error, recheck,
    },
    openapi::problem_responses,
    organization::present,
    require::{ItemTarget, ModuleTarget, Require, code::ITEM_NOT_FOUND},
};

/// The path of an item; `{ITEMS_PATH}/{item_id}`.
pub const ITEMS_PATH: &str = "/api/v1/items";

problem_responses! {
    ItemNotFound => "`item.not_found`: no item with this id exists in the organization, or you cannot see its course. `org.not_found`: the request's organization cannot be determined.",
    ItemMoveNotFound => "`item.not_found`: no item with this id exists in the organization, or you cannot see its course. `module.not_found`: the destination module is not in the item's course. `org.not_found`: the request's organization cannot be determined.",
    ItemCreateConflict => "`course.archived`: the course is archived and takes no content change. `item.limit_reached`: the module holds 200 items.",
    ItemOrderConflict => "`course.archived`: the course is archived and takes no content change. `order.stale`: the list is not exactly the current set of ids (one is missing, extra or repeated); reload and try again.",
    ItemWriteConflict => "`course.archived`: the course is archived and takes no content change.",
    ItemMoveConflict => "`course.archived`: the course is archived and takes no content change. `item.limit_reached`: the destination module holds 200 items.",
    ItemInvalid => "`request.invalid`: `errors` names `/title` (`name.empty`, `name.too_long`, `text.contains_nul`), `/kind` with `item.kind_unavailable` (this release cannot create items of the kind yet), `/url` with `item.url_invalid` (not an absolute `https` or `http` URL of at most 2,048 characters with a host, without a user name, password, white space or control character), `item.url_required` (an external link without a URL) or `item.url_unavailable` (a URL on another kind), `/ref_id` with `item.ref_id_unavailable` (a reference on a kind that has none), or `/position` with `item.position_out_of_range` (outside 0 to the module's item count).",
}

/// An item to create, unpublished.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewItem {
    /// `header` and `external_link` can be created; the other kinds arrive with their specs.
    pub kind: ItemKind,
    /// Trimmed; 1 to 200 characters.
    pub title: String,
    /// What the item points at, for a kind that points at a row.
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
        (status = 404, response = ModuleNotFound),
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
    if let Some(url) = &body.url {
        check_url(url)?;
    }
    let mut tx = begin(&state).await?;
    recheck(&mut tx, grants, target.course.id, MODULE_NOT_FOUND).await?;
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
    recheck(&mut tx, grants, target.course.id, MODULE_NOT_FOUND).await?;
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

/// Rename, publish or unpublish an item, or change an external link's URL. A patch naming only
/// current values changes nothing.
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
    if let Some(url) = &body.url {
        check_url(url)?;
    }
    let mut tx = begin(&state).await?;
    recheck(&mut tx, grants, target.course.id, ITEM_NOT_FOUND).await?;
    let patch = items::ItemPatch {
        title: body.title,
        published: body.published,
        url: body.url,
    };
    let item = items::update(
        &mut *tx,
        grants.scope(),
        target.id,
        &patch,
        state.clock.now(),
    )
    .await
    .map_err(content_error)?;
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
    recheck(&mut tx, grants, target.course.id, ITEM_NOT_FOUND).await?;
    let (scope, now) = (grants.scope(), state.clock.now());
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
    commit(tx).await?;
    Ok(Json(Item::from_repo(item)?))
}

/// Delete an item. What it points at stays in the course.
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
        (status = 409, response = ItemWriteConflict),
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
    recheck(&mut tx, grants, target.course.id, ITEM_NOT_FOUND).await?;
    items::delete(&mut *tx, grants.scope(), target.id)
        .await
        .map_err(content_error)?;
    commit(tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 422 `item.url_invalid` at `/url` unless `url` is an absolute `https` or `http` URL of at
/// most 2,048 characters with a host, without a user name, password, white space or control
/// character (B7). The URL is stored as sent, so it must parse as written: the URL parser would
/// drop blanks, tabs and newlines and read `https:host` as `https://host`, which a browser
/// resolves differently against the page.
fn check_url(url: &str) -> Result<(), ApiError> {
    if url_is_valid(url) {
        Ok(())
    } else {
        Err(ApiError::invalid(vec![FieldError::at(
            &["url"],
            code::ITEM_URL_INVALID,
        )]))
    }
}

fn url_is_valid(url: &str) -> bool {
    let plain = !url.chars().any(|c| c.is_whitespace() || c.is_control());
    let length = url.chars().count();
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    let absolute = lower.starts_with("https://") || lower.starts_with("http://");
    let parsed = Url::parse(url).ok().filter(|parsed| {
        matches!(parsed.scheme(), "https" | "http")
            && parsed.host_str().is_some_and(|host| !host.is_empty())
            && parsed.username().is_empty()
            && parsed.password().is_none()
    });
    plain && absolute && (1..=items::MAX_URL_CHARS).contains(&length) && parsed.is_some()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn t5_urls_b7_refuses() {
        let long = format!("https://example.com/{}", "a".repeat(2049 - 20));
        assert_eq!(long.chars().count(), 2049);
        for bad in [
            "javascript:alert(1)",
            "//evil.example",
            "https://user:pw@host/",
            "https://user@host/",
            "https://exa\tmple.com/",
            " https://example.com/",
            "https://example.com/\n",
            "https:example.com",
            "ftp://example.com/",
            "data:text/html,hi",
            "/relative",
            "",
            long.as_str(),
        ] {
            assert!(!url_is_valid(bad), "{bad:?}");
        }
    }

    #[test]
    fn t5_urls_b7_accepts() {
        let at_limit = format!("https://example.com/{}", "a".repeat(2048 - 20));
        for good in [
            "https://example.com/a?b=1",
            "http://example.com",
            "HTTPS://Example.com/Path#frag",
            "https://[2001:db8::1]:8443/x",
            at_limit.as_str(),
        ] {
            assert!(url_is_valid(good), "{good:?}");
        }
    }

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
