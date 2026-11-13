// SPDX-License-Identifier: AGPL-3.0-or-later
//! The account tree under `/api/v1/accounts`
//! (docs/specs/m0/02-organizations-and-accounts.md B18 to B25 and API).
//!
//! Account permissions apply to a subtree (B23): a grant at account A covers A and every account
//! below it, and a grant at the organization covers every account. Reading needs `account.view`
//! over the account; creating needs `account.manage` over the new account's parent; renaming
//! needs it over the account; moving and deleting need it over the account's parent, so a holder
//! at A reaches only A's strict descendants, and a move also needs it over the destination. A
//! destination outside every subtree where the caller holds `account.manage` answers 404, as an
//! id the caller cannot see does. A move may not give the caller a permission over the moved
//! account it lacks there now (403 `permission.escalation`): every grant on the destination's
//! path contains the account after the move. Deleting refuses an account that role assignments
//! are scoped to; the role-assignment routes remove them.
//!
//! The `Require` extractor checks the path's account before the handler runs. Create, move and
//! delete then check again, inside the transaction that holds the organization's tree lock
//! (`stepclimb_core::accounts::lock_tree`), on the paths read under that lock, and write in the
//! same transaction: a concurrent move that commits between the extractor's check and the write
//! cannot carry the account, its parent or the destination out of the caller's reach unseen.
//! Renames take no tree lock (B22): they read the account row `FOR UPDATE`, which waits for a
//! move in progress over it, check its path again and rename in the same transaction.

use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, Postgres, Transaction};
use stepclimb_core::{
    OrgScope,
    accounts::{self, Reach},
    grants::{Denied, Grants, ScopeId, Target},
    id::Uuid,
    perm::{self, AccountManage, AccountView},
};
use utoipa::{IntoParams, ToSchema};

use crate::{
    AppState,
    audit::ClientAddress,
    error::{ApiError, FieldError},
    extract::{ApiJson, ApiQuery},
    openapi::problem_responses,
    organization::present,
    pagination::{Page, PageParams},
    require::{AccountParentTarget, AccountTarget, AnyScope, Require, check_at_parent},
    roles::code::{NAME_EMPTY, NAME_TOO_LONG, PERMISSION_ESCALATION, TEXT_CONTAINS_NUL},
    timestamp,
};

/// The path of the accounts collection; an account is at `{ACCOUNTS_PATH}/{account_id}`.
pub const ACCOUNTS_PATH: &str = "/api/v1/accounts";

/// The codes this module produces besides `account.not_found` (`require::code`).
pub mod code {
    /// The root account cannot be moved or deleted (409).
    pub const ACCOUNT_ROOT_IMMUTABLE: &str = "account.root_immutable";
    /// The new parent is the account itself or one of its descendants (409).
    pub const ACCOUNT_MOVE_INTO_DESCENDANT: &str = "account.move_into_descendant";
    /// The account still has child accounts (409).
    pub const ACCOUNT_HAS_CHILDREN: &str = "account.has_children";
    /// Role assignments are still scoped to the account (409).
    pub const ACCOUNT_HAS_ASSIGNMENTS: &str = "account.has_assignments";
    /// The create or move would put an account more than 16 levels below the root (409).
    pub const ACCOUNT_TOO_DEEP: &str = "account.too_deep";
}

problem_responses! {
    AccountNotFound => "`account.not_found`: no account with this id exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined.",
    ParentNotFound => "`account.not_found`: no account with `parent_id` exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined.",
    MoveNotFound => "`account.not_found`: the account does not exist or you cannot see its parent, or the destination `parent_id` does not exist or is outside every subtree where you hold `account.manage`. `org.not_found`: the request's organization cannot be determined.",
    DeleteNotFound => "`account.not_found`: the account does not exist or you cannot see its parent. `org.not_found`: the request's organization cannot be determined.",
    CreateForbidden => "`permission.denied`: you hold `account.manage` nowhere, or you can see the parent without holding it there. `auth.csrf_refused`: the request did not come from this site.",
    ManageForbidden => "`permission.denied`: you can see the account but do not hold `account.manage` at a scope containing it (for a delete, the account's parent). `auth.csrf_refused`: the request did not come from this site.",
    MoveForbidden => "`permission.denied`: you can see the account's parent but do not hold `account.manage` there. `permission.escalation`: the destination would give you, over the moved account, a permission you do not hold over it now. `auth.csrf_refused`: the request did not come from this site.",
    CreateConflict => "`account.too_deep`: the new account would be more than 16 levels below the root.",
    MoveConflict => "`account.root_immutable`: the root account cannot be moved. `account.move_into_descendant`: the destination is the account itself or one of its descendants. `account.too_deep`: an account of the moved subtree would end more than 16 levels below the root.",
    DeleteConflict => "`account.root_immutable`: the root account cannot be deleted. `account.has_children`: the account has child accounts; move or delete them first. `account.has_assignments`: role assignments are scoped to the account; remove them first.",
}

/// An account of the caller's organization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Account {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    /// `null` for the root only. Shown even when the parent is not visible to the caller.
    #[schema(value_type = Option<String>, format = "uuid", required = true)]
    pub parent_id: Option<Uuid>,
    pub name: String,
    /// 0 for the root, 1 for its children, and so on; at most 16.
    pub depth: i32,
    pub has_children: bool,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub updated_at: String,
}

impl Account {
    fn from_repo(account: accounts::Account) -> Result<Self, ApiError> {
        let time = |t| timestamp::rfc3339(t).map_err(|error| ApiError::internal(&error));
        Ok(Self {
            created_at: time(account.created_at)?,
            updated_at: time(account.updated_at)?,
            id: account.id,
            parent_id: account.parent_id,
            name: account.name,
            depth: account.depth,
            has_children: account.has_children,
        })
    }
}

/// An account to create.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewAccount {
    /// The parent account; you must hold `account.manage` at a scope containing it.
    #[schema(value_type = String, format = "uuid")]
    pub parent_id: Uuid,
    /// Trimmed; 1 to 200 characters.
    pub name: String,
}

/// A change to an account; an omitted member is left as it is. A member sent as `null` is
/// malformed: omit it instead.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountPatch {
    /// Trimmed; 1 to 200 characters.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub name: Option<String>,
}

/// The new parent of a moved account.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountMove {
    /// The new parent; you must hold `account.manage` at a scope containing it. The account's
    /// current parent changes nothing.
    #[schema(value_type = String, format = "uuid")]
    pub parent_id: Uuid,
}

/// The filter of the accounts list.
#[derive(Clone, Copy, Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AccountListParams {
    /// List this account's children. Omitted, list the topmost accounts you can see with
    /// `account.view`.
    #[param(value_type = Option<String>, format = "uuid")]
    pub parent_id: Option<Uuid>,
}

/// List an account's children, or without `parent_id` the topmost accounts the caller can see:
/// the root for a holder of `account.view` at organization scope, otherwise each account where
/// the caller holds it below no other such account. Sorted by name, then id.
#[utoipa::path(
    get,
    path = "/api/v1/accounts",
    operation_id = "listAccounts",
    tag = "accounts",
    params(AccountListParams, PageParams),
    responses(
        (status = 200, description = "One page of accounts", body = Page<Account>),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = ParentNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_accounts(
    require: Require<AccountView, AnyScope>,
    State(state): State<AppState>,
    ApiQuery(filter): ApiQuery<AccountListParams>,
    ApiQuery(params): ApiQuery<PageParams>,
) -> Result<Json<Page<Account>>, ApiError> {
    let page = params.parse::<accounts::ListKey>()?;
    let scope = require.principal().scope();
    let (after, limit) = (page.after(), page.fetch_limit());
    let rows = match filter.parent_id {
        Some(parent_id) => {
            require
                .grants()
                .require_at::<AccountView>(&state.db, ScopeId::Account(parent_id))
                .await?;
            accounts::list_children(&state.db, scope, parent_id, after, limit).await
        }
        None => {
            let reach = top_reach(require.grants());
            let reach = match &reach {
                Some(ids) => Reach::Accounts(ids),
                None => Reach::Organization,
            };
            accounts::list_top_visible(&state.db, scope, reach, after, limit).await
        }
    }
    .map_err(|error| match error {
        // Only the cursor's name is a caller's string here.
        accounts::Error::ContainsNul { .. } => ApiError::invalid_cursor(),
        other => account_error(other),
    })?;
    let rows = rows
        .into_iter()
        .map(Account::from_repo)
        .collect::<Result<Vec<_>, _>>()?;
    page.page(rows, |account| (account.name.clone(), account.id))
        .map(Json)
}

/// The accounts where the caller holds `account.view`, or `None` when it holds it at
/// organization scope, which reaches every account.
fn top_reach(grants: &Grants) -> Option<Vec<Uuid>> {
    let mut ids = Vec::new();
    for scope in grants.scopes_with::<AccountView>() {
        match scope {
            ScopeId::Organization(_) => return None,
            ScopeId::Account(id) => ids.push(id),
        }
    }
    Some(ids)
}

/// Read one account.
#[utoipa::path(
    get,
    path = "/api/v1/accounts/{account_id}",
    operation_id = "getAccount",
    tag = "accounts",
    params(("account_id" = String, Path, format = "uuid", description = "The account's id.")),
    responses(
        (status = 200, description = "The account", body = Account),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = AccountNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_account(
    require: Require<AccountView, AccountTarget>,
    State(state): State<AppState>,
) -> Result<Json<Account>, ApiError> {
    let account = accounts::get(&state.db, require.principal().scope(), require.target().id)
        .await
        .map_err(account_error)?
        .ok_or_else(|| ApiError::from(Denied::AccountNotFound))?;
    Ok(Json(Account::from_repo(account)?))
}

/// Create an account under `parent_id`.
#[utoipa::path(
    post,
    path = "/api/v1/accounts",
    operation_id = "createAccount",
    tag = "accounts",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = NewAccount,
    responses(
        (
            status = 201,
            description = "The account is created",
            body = Account,
            headers(("Location" = String, description = "The account's path"))
        ),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = CreateForbidden),
        (status = 404, response = ParentNotFound),
        (status = 409, response = CreateConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn create_account(
    require: Require<AccountManage, AnyScope>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<NewAccount>,
) -> Result<Response, ApiError> {
    let scope = require.principal().scope();
    let mut tx = locked_tree(&state, scope).await?;
    require
        .grants()
        .require_at::<AccountManage>(&mut *tx, ScopeId::Account(body.parent_id))
        .await?;
    let now = state.clock.now();
    let actor = client.actor(require.principal());
    let account = accounts::create(&mut *tx, scope, body.parent_id, &body.name, now, actor)
        .await
        .map_err(account_error)?;
    commit(tx).await?;
    let location = format!("{ACCOUNTS_PATH}/{}", account.id);
    let account = Account::from_repo(account)?;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(account),
    )
        .into_response())
}

/// Rename an account. A patch without `name` changes nothing.
#[utoipa::path(
    patch,
    path = "/api/v1/accounts/{account_id}",
    operation_id = "updateAccount",
    tag = "accounts",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("account_id" = String, Path, format = "uuid", description = "The account's id.")),
    request_body = AccountPatch,
    responses(
        (status = 200, description = "The account after the change", body = Account),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ManageForbidden),
        (status = 404, response = AccountNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_account(
    require: Require<AccountManage, AccountTarget>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<AccountPatch>,
) -> Result<Json<Account>, ApiError> {
    let (scope, id) = (require.principal().scope(), require.target().id);
    let mut tx = begin(&state).await?;
    // The extractor's check again, on the path as the locked row holds it: a move committed
    // since cannot have carried the account out of the caller's reach unseen.
    let path = accounts::lock_path(&mut tx, scope, id)
        .await
        .map_err(account_error)?
        .ok_or(Denied::AccountNotFound)?;
    require
        .grants()
        .check::<AccountManage>(&Target::Account(path))?;
    let account = match body.name {
        Some(name) => {
            let actor = client.actor(require.principal());
            accounts::rename(&mut *tx, scope, id, &name, state.clock.now(), actor).await
        }
        None => accounts::get(&mut *tx, scope, id)
            .await
            .and_then(|found| found.ok_or(accounts::Error::NotFound)),
    }
    .map_err(account_error)?;
    commit(tx).await?;
    Ok(Json(Account::from_repo(account)?))
}

/// Move an account and its subtree under `parent_id`.
#[utoipa::path(
    post,
    path = "/api/v1/accounts/{account_id}/move",
    operation_id = "moveAccount",
    tag = "accounts",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("account_id" = String, Path, format = "uuid", description = "The account's id.")),
    request_body = AccountMove,
    responses(
        (status = 200, description = "The account after the move", body = Account),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = MoveForbidden),
        (status = 404, response = MoveNotFound),
        (status = 409, response = MoveConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn move_account(
    require: Require<AccountManage, AccountParentTarget>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<AccountMove>,
) -> Result<Json<Account>, ApiError> {
    let (scope, id) = (require.principal().scope(), require.target().id);
    let mut tx = locked_tree(&state, scope).await?;
    let from = check_movable(&mut tx, require.grants(), id).await?;
    let to = check_destination(&mut tx, require.grants(), body.parent_id).await?;
    refuse_escalation(require.grants(), id, &from, &to)?;
    let now = state.clock.now();
    let actor = client.actor(require.principal());
    let moved = accounts::move_to(&mut *tx, scope, id, body.parent_id, now, actor)
        .await
        .map_err(account_error)?;
    commit(tx).await?;
    Ok(Json(Account::from_repo(moved)?))
}

/// Delete an account that has no child accounts, and the role assignments scoped to it.
#[utoipa::path(
    delete,
    path = "/api/v1/accounts/{account_id}",
    operation_id = "deleteAccount",
    tag = "accounts",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("account_id" = String, Path, format = "uuid", description = "The account's id.")),
    responses(
        (status = 204, description = "The account is deleted"),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = ManageForbidden),
        (status = 404, response = DeleteNotFound),
        (status = 409, response = DeleteConflict),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn delete_account(
    require: Require<AccountManage, AccountParentTarget>,
    client: ClientAddress,
    State(state): State<AppState>,
) -> Result<StatusCode, ApiError> {
    let (scope, id) = (require.principal().scope(), require.target().id);
    let mut tx = locked_tree(&state, scope).await?;
    check_movable(&mut tx, require.grants(), id).await?;
    let actor = client.actor(require.principal());
    accounts::delete(&mut *tx, scope, id, state.clock.now(), actor)
        .await
        .map_err(account_error)?;
    commit(tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn begin(state: &AppState) -> Result<Transaction<'static, Postgres>, ApiError> {
    state
        .db
        .begin()
        .await
        .map_err(|error| ApiError::internal(&error))
}

/// A transaction holding the scope's tree lock, for checks that must still hold at the write.
async fn locked_tree(
    state: &AppState,
    scope: OrgScope,
) -> Result<Transaction<'static, Postgres>, ApiError> {
    let mut tx = begin(state).await?;
    accounts::lock_tree(&mut tx, scope)
        .await
        .map_err(account_error)?;
    Ok(tx)
}

async fn commit(tx: Transaction<'static, Postgres>) -> Result<(), ApiError> {
    tx.commit()
        .await
        .map_err(|error| ApiError::internal(&error))
}

/// The extractor's `AccountParentTarget` check again, on the account's path as the locked
/// transaction reads it: 404 when the account is gone or its parent is out of the caller's
/// reach, 403 when the parent is visible without `account.manage`, and 409 for the root, which
/// only a holder at organization scope gets this far with. Returns the account's path.
async fn check_movable(
    conn: &mut PgConnection,
    grants: &Grants,
    id: Uuid,
) -> Result<Vec<Uuid>, ApiError> {
    let path = accounts::ancestors_path(&mut *conn, grants.scope(), id)
        .await
        .map_err(account_error)?
        .ok_or(Denied::AccountNotFound)?;
    match check_at_parent::<AccountManage>(grants, path.clone())? {
        Some(_) => Ok(path),
        None => Err(account_error(accounts::Error::RootImmutable)),
    }
}

/// The destination of a move, as the locked transaction reads it: 404 unless it exists and the
/// caller holds `account.manage` at a scope containing it (B23). Returns its path.
async fn check_destination(
    conn: &mut PgConnection,
    grants: &Grants,
    parent_id: Uuid,
) -> Result<Vec<Uuid>, ApiError> {
    let path = accounts::ancestors_path(&mut *conn, grants.scope(), parent_id)
        .await
        .map_err(account_error)?
        .ok_or(Denied::AccountNotFound)?;
    if grants.holds::<AccountManage>(&Target::Account(path.clone())) {
        Ok(path)
    } else {
        Err(Denied::AccountNotFound.into())
    }
}

/// 403 `permission.escalation` when moving the account `id` (path `from`) under the account at
/// `to` would give the caller a permission over it they do not hold over it now (B23): after
/// the move, every grant on the destination's path contains the account and its subtree. A
/// destination inside the account's own subtree is left to the repository's 409.
fn refuse_escalation(
    grants: &Grants,
    id: Uuid,
    from: &[Uuid],
    to: &[Uuid],
) -> Result<(), ApiError> {
    if to.contains(&id) {
        return Ok(());
    }
    let (now, after) = (Target::Account(from.to_vec()), Target::Account(to.to_vec()));
    let gained = perm::ALL
        .iter()
        .any(|p| grants.holds_key(p.key(), &after) && !grants.holds_key(p.key(), &now));
    if !gained {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::FORBIDDEN,
        PERMISSION_ESCALATION,
        "The destination would give you permissions over this account that you do not hold over it now.",
    ))
}

/// The answer for an accounts repository error.
fn account_error(error: accounts::Error) -> ApiError {
    use accounts::Error as E;
    let invalid = |path: &[&str], code| ApiError::invalid(vec![FieldError::at(path, code)]);
    let conflict = |code, detail| ApiError::new(StatusCode::CONFLICT, code, detail);
    match error {
        E::NotFound | E::ParentNotFound => Denied::AccountNotFound.into(),
        E::NameEmpty => invalid(&["name"], NAME_EMPTY),
        E::NameTooLong => invalid(&["name"], NAME_TOO_LONG),
        E::ContainsNul { field } => invalid(&[field], TEXT_CONTAINS_NUL),
        E::RootImmutable => conflict(
            code::ACCOUNT_ROOT_IMMUTABLE,
            "The root account cannot be moved or deleted.",
        ),
        E::MoveIntoDescendant => conflict(
            code::ACCOUNT_MOVE_INTO_DESCENDANT,
            "An account cannot move under itself or one of its descendants.",
        ),
        E::HasChildren => conflict(
            code::ACCOUNT_HAS_CHILDREN,
            "The account has child accounts. Move or delete them first.",
        ),
        E::HasAssignments => conflict(
            code::ACCOUNT_HAS_ASSIGNMENTS,
            "Role assignments are scoped to the account. Remove them first.",
        ),
        E::TooDeep => conflict(
            code::ACCOUNT_TOO_DEEP,
            "Accounts cannot be nested more than 16 levels below the root.",
        ),
        other
        @ (E::IsolationLevel { .. } | E::CorruptPath { .. } | E::Time(_) | E::Database(_)) => {
            ApiError::internal(&other)
        }
    }
}

#[cfg(test)]
mod tests {
    use stepclimb_core::{
        accounts::MAX_DEPTH,
        id::new_id,
        role_assignments::{GrantScope, UserGrant},
    };

    use super::*;

    #[expect(
        clippy::disallowed_methods,
        reason = "unit tests of the reach need grants and have no credential"
    )]
    fn grants(scopes: Vec<GrantScope>) -> Grants {
        let grant = |scope| UserGrant {
            assignment_id: new_id(),
            role_id: new_id(),
            role_key: "custom".into(),
            permissions: vec!["account.view".into()],
            scope,
        };
        Grants::from_assignments(OrgScope::instance(new_id()), scopes.into_iter().map(grant))
    }

    #[test]
    fn the_top_reach_is_the_organization_or_each_viewing_account() {
        let (tower, enroute) = (new_id(), new_id());
        let at = |id| GrantScope::Account { id, path: vec![id] };
        assert_eq!(
            top_reach(&grants(vec![GrantScope::Organization, at(tower)])),
            None
        );
        let mut both = top_reach(&grants(vec![at(tower), at(enroute)])).unwrap();
        both.sort();
        let mut expected = vec![tower, enroute];
        expected.sort();
        assert_eq!(both, expected);
        assert_eq!(top_reach(&grants(vec![])), Some(vec![]));
    }

    #[test]
    fn repository_errors_map_to_their_status_code_and_field() {
        let cases = [
            (accounts::Error::NotFound, 404, "account.not_found", None),
            (
                accounts::Error::ParentNotFound,
                404,
                "account.not_found",
                None,
            ),
            (
                accounts::Error::RootImmutable,
                409,
                "account.root_immutable",
                None,
            ),
            (
                accounts::Error::MoveIntoDescendant,
                409,
                "account.move_into_descendant",
                None,
            ),
            (
                accounts::Error::HasChildren,
                409,
                "account.has_children",
                None,
            ),
            (accounts::Error::TooDeep, 409, "account.too_deep", None),
            (
                accounts::Error::HasAssignments,
                409,
                "account.has_assignments",
                None,
            ),
            (
                accounts::Error::NameEmpty,
                422,
                "request.invalid",
                Some(("/name", "name.empty")),
            ),
            (
                accounts::Error::NameTooLong,
                422,
                "request.invalid",
                Some(("/name", "name.too_long")),
            ),
            (
                accounts::Error::ContainsNul { field: "name" },
                422,
                "request.invalid",
                Some(("/name", "text.contains_nul")),
            ),
            (
                accounts::Error::CorruptPath { id: new_id() },
                500,
                "internal.error",
                None,
            ),
            (
                accounts::Error::IsolationLevel {
                    found: "serializable".into(),
                },
                500,
                "internal.error",
                None,
            ),
        ];
        for (error, status, code, field) in cases {
            let problem = account_error(error).problem();
            assert_eq!((problem.status, problem.code.as_str()), (status, code));
            let fields: Vec<_> = problem
                .errors
                .iter()
                .map(|e| (e.field.as_str(), e.code.as_str()))
                .collect();
            assert_eq!(fields, field.into_iter().collect::<Vec<_>>(), "{code}");
        }
    }

    #[test]
    fn the_documented_depth_limit_is_the_repositorys() {
        assert_eq!(MAX_DEPTH, 16);
        assert!(code::ACCOUNT_TOO_DEEP.starts_with("account."));
        let detail = account_error(accounts::Error::TooDeep).problem().detail;
        assert!(detail.contains(&format!("{MAX_DEPTH} levels")), "{detail}");
    }

    #[test]
    fn a_patch_refuses_null_and_reads_an_omitted_name_as_none() {
        use serde_json::json;
        assert!(serde_json::from_value::<AccountPatch>(json!({ "name": null })).is_err());
        let omitted: AccountPatch = serde_json::from_value(json!({})).unwrap();
        assert_eq!(omitted.name, None);
        assert!(serde_json::from_value::<AccountMove>(json!({ "parent_id": "x" })).is_err());
    }
}
