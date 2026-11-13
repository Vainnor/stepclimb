// SPDX-License-Identifier: AGPL-3.0-or-later
//! The user admin routes under `/api/v1/users`
//! (docs/specs/m0/03-users-and-identities.md B3, B6, B7, B9, B10, B12 and API).
//!
//! Every route takes the organization as its target: reading needs `user.view` and writing
//! `user.manage` at organization scope, so a `user.view` grant at an account does not reach them
//! (spec 09 Q2). A user id of another organization is 404 `user.not_found`, as an id that does
//! not exist.
//!
//! `POST` creates a `local`, `active` user. In the transaction that inserts it, it first runs
//! the users limit check ([`Entitlements::check_room`], spec 10 B10), which locks the
//! organization's plan row so concurrent creations take turns; a user without a `tz` gets the
//! organization's `default_tz` setting (Q4); each of `identities` is linked to the new user, and
//! any refusal rolls back the whole creation. Creation writes no audit entry (spec 09 Q5 lists
//! status changes only).
//!
//! `PATCH` refuses, for a user an outside source owns (source other than `local`), any change
//! to the fields that source owns: `display_name`, `given_name`, `family_name` and `email` (409
//! `user.externally_managed`, B6). Locale, time zone and status stay the organization's: an
//! admin can suspend such a user at once (Q3). Suspending yourself is refused (409
//! `user.cannot_suspend_self`), and suspending the organization's last active Organization admin
//! is refused by `stepclimb_core::users::set_status` (409 `role_assignment.last_org_admin`),
//! which writes the `user.set_status` audit entry. The field changes and the status change run
//! in one transaction, so a refused status leaves the fields as they were.

use std::time::SystemTime;

use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Map;
use sqlx::{Postgres, Transaction};
use stepclimb_core::{
    OrgScope, Source,
    feature::Limit,
    id::Uuid,
    identities::{self, NewIdentity},
    identity_providers, organizations,
    perm::{UserManage, UserView},
    users::{self as repo, Status},
};
use utoipa::{IntoParams, ToSchema};

use crate::{
    AppState,
    audit::ClientAddress,
    entitlements::Entitlements,
    error::{ApiError, FieldError},
    extract::{ApiJson, ApiPath, ApiQuery},
    me::{UserSource, UserStatus},
    openapi::problem_responses,
    organization::present,
    pagination::{Page, PageParams},
    require::Require,
    role_assignments::code::{ROLE_ASSIGNMENT_LAST_ORG_ADMIN, USER_NOT_FOUND},
    roles::code::{NAME_EMPTY, TEXT_CONTAINS_NUL},
    timestamp,
};

/// The path of the users collection; a user is at `{USERS_PATH}/{user_id}`.
pub const USERS_PATH: &str = "/api/v1/users";

/// The most identities one `POST` may pre-provision.
pub const MAX_NEW_IDENTITIES: usize = 20;

/// The codes this module produces besides `user.not_found` (`role_assignments::code`).
pub mod code {
    /// An outside source owns the user's names and email (409).
    pub const USER_EXTERNALLY_MANAGED: &str = "user.externally_managed";
    /// The caller tried to suspend themselves (409).
    pub const USER_CANNOT_SUSPEND_SELF: &str = "user.cannot_suspend_self";
    /// Another user of the organization has the external id (409).
    pub const USER_EXTERNAL_ID_TAKEN: &str = "user.external_id_taken";
    /// Another user of the organization has the email, compared case-insensitively (409).
    pub const USER_EMAIL_TAKEN: &str = "user.email_taken";
    /// An identity to pre-provision is already linked to a user (409).
    pub const IDENTITY_TAKEN: &str = "identity.taken";
    /// In a 422's `errors`: the locale is not supported (only `en` in M0).
    pub const USER_LOCALE_UNSUPPORTED: &str = "user.locale_unsupported";
    /// In a 422's `errors`: no identity provider of the organization has this key.
    pub const IDENTITY_UNKNOWN_PROVIDER: &str = "identity.unknown_provider";
    /// In a 422's `errors`: the subject is empty.
    pub const IDENTITY_SUBJECT_EMPTY: &str = "identity.subject_empty";
    /// In a 422's `errors`: more identities than [`super::MAX_NEW_IDENTITIES`].
    pub const IDENTITIES_TOO_MANY: &str = "identities.too_many";
    /// In a 422's `errors`: the time zone is no time zone of the IANA database.
    pub const TZ_UNKNOWN: &str = "tz.unknown";
    /// In a 422's `errors`: the email is empty once trimmed.
    pub const EMAIL_EMPTY: &str = "email.empty";
    /// In a 422's `errors`: the email holds a control or invisible formatting character.
    pub const EMAIL_INVALID_CHARACTER: &str = "email.invalid_character";
    /// In a 422's `errors`: the string is longer than its limit.
    pub const TEXT_TOO_LONG: &str = "text.too_long";
}

problem_responses! {
    UserNotFound => "`user.not_found`: no user of the organization has this id. `org.not_found`: the request's organization cannot be determined.",
    UserWriteForbidden => "`permission.denied`: the caller does not hold `user.manage` at organization scope. `auth.csrf_refused`: the request did not come from this site.",
    UserCreateForbidden => "`permission.denied`: the caller does not hold `user.manage` at organization scope. `entitlement.limit_reached`: the organization's plan allows no more users; `limit` and `max` name the limit and its value. `auth.csrf_refused`: the request did not come from this site.",
    UserCreateConflict => "`user.external_id_taken`: another user of the organization has this external id. `user.email_taken`: another user of the organization has this email, in any letter case. `identity.taken`: one of `identities` is already linked to a user.",
    UserUpdateConflict => "`user.externally_managed`: an identity provider, the sync API, SCIM or a connector owns the user's names and email; change them there. `user.cannot_suspend_self`: you cannot suspend yourself. `user.email_taken`: another user of the organization has this email, in any letter case. `role_assignment.last_org_admin`: the user is the organization's last active Organization admin.",
}

/// A user of the caller's organization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct User {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub display_name: String,
    /// `""` when none was given.
    pub given_name: String,
    /// `""` when none was given.
    pub family_name: String,
    #[schema(required = true)]
    pub email: Option<String>,
    /// `en` in M0.
    pub locale: String,
    /// An IANA time zone.
    pub tz: String,
    pub status: UserStatus,
    pub source: UserSource,
    /// The owning system's id for the user; `null` when there is none.
    #[schema(required = true)]
    pub external_id: Option<String>,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub created_at: String,
    /// RFC 3339, UTC.
    #[schema(format = DateTime)]
    pub updated_at: String,
}

impl User {
    fn from_repo(user: repo::User) -> Result<Self, ApiError> {
        let time = |t| timestamp::rfc3339(t).map_err(|error| ApiError::internal(&error));
        Ok(Self {
            created_at: time(user.created_at)?,
            updated_at: time(user.updated_at)?,
            id: user.id,
            display_name: user.display_name,
            given_name: user.given_name,
            family_name: user.family_name,
            email: user.email,
            locale: user.locale,
            tz: user.tz,
            status: user.status.into(),
            source: user.source.into(),
            external_id: user.external_id,
        })
    }
}

/// A user to create. The new user is `local` and `active`, and has no local credential.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewUser {
    /// Trimmed; 1 to 200 characters.
    pub display_name: String,
    /// At most 200 characters; `""` when omitted.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub given_name: Option<String>,
    /// At most 200 characters; `""` when omitted.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub family_name: Option<String>,
    /// Trimmed; at most 254 characters; unique in the organization in any letter case.
    #[serde(default)]
    pub email: Option<String>,
    /// `en`, the default and the only locale in M0.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub locale: Option<String>,
    /// An IANA time zone; the organization's `default_tz` when omitted.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub tz: Option<String>,
    /// At most 200 characters; unique in the organization.
    #[serde(default)]
    pub external_id: Option<String>,
    /// Identities to link to the new user, so they can sign in with them the first time; at
    /// most 20.
    #[serde(default)]
    #[schema(max_items = 20)]
    pub identities: Vec<NewUserIdentity>,
}

/// An identity to link to a new user: the provider's subject at the provider with this key.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewUserIdentity {
    pub provider_key: String,
    /// Stored exactly as given; not empty.
    pub subject: String,
}

/// A change to a user; an omitted member is left as it is. A member sent as `null` is
/// malformed, except `email`, where `null` removes the email.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UserPatch {
    /// Trimmed; 1 to 200 characters. Refused for a user an outside source owns.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub display_name: Option<String>,
    /// At most 200 characters. Refused for a user an outside source owns.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub given_name: Option<String>,
    /// At most 200 characters. Refused for a user an outside source owns.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub family_name: Option<String>,
    /// Trimmed; at most 254 characters; `null` removes it. Refused for a user an outside source
    /// owns.
    #[serde(default, deserialize_with = "nullable")]
    #[schema(value_type = Option<String>)]
    pub email: Option<Option<String>>,
    /// `en` in M0.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub locale: Option<String>,
    /// An IANA time zone.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub tz: Option<String>,
    /// `suspended` ends every session of the user on its next request; you cannot suspend
    /// yourself.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = UserStatus)]
    pub status: Option<UserStatus>,
}

/// A member that may be `null`: present, it reads as `Some(value)` with `value` `None` for
/// `null`; omitted, it reads as `None` through `#[serde(default)]`.
fn nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

impl UserPatch {
    /// The repository's patch: every member but `status`.
    fn fields(&self) -> repo::UserPatch {
        repo::UserPatch {
            display_name: self.display_name.clone(),
            given_name: self.given_name.clone(),
            family_name: self.family_name.clone(),
            email: self.email.clone(),
            locale: self.locale.clone(),
            tz: self.tz.clone(),
        }
    }

    /// Whether the patch names a field an outside source owns (B6).
    fn names_source_owned_field(&self) -> bool {
        self.display_name.is_some()
            || self.given_name.is_some()
            || self.family_name.is_some()
            || self.email.is_some()
    }
}

/// The filters of the users list; each narrows it.
#[derive(Clone, Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct UserListParams {
    pub status: Option<UserStatus>,
    pub source: Option<UserSource>,
    /// A prefix of the display name or the email, in any letter case.
    pub q: Option<String>,
}

/// The path parameter of a single user's route.
#[derive(Clone, Copy, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Path)]
pub struct UserIdParam {
    /// The user's id.
    #[param(value_type = String, format = "uuid")]
    pub user_id: Uuid,
}

impl From<UserStatus> for Status {
    fn from(status: UserStatus) -> Self {
        match status {
            UserStatus::Active => Self::Active,
            UserStatus::Suspended => Self::Suspended,
        }
    }
}

impl From<UserSource> for Source {
    fn from(source: UserSource) -> Self {
        match source {
            UserSource::Local => Self::Local,
            UserSource::Idp => Self::Idp,
            UserSource::Sync => Self::Sync,
            UserSource::Scim => Self::Scim,
            UserSource::Connector => Self::Connector,
        }
    }
}

/// List the organization's users, sorted by display name, then id.
#[utoipa::path(
    get,
    path = "/api/v1/users",
    operation_id = "listUsers",
    tag = "users",
    params(UserListParams, PageParams),
    responses(
        (status = 200, description = "One page of users", body = Page<User>),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn list_users(
    require: Require<UserView>,
    State(state): State<AppState>,
    ApiQuery(filter): ApiQuery<UserListParams>,
    ApiQuery(params): ApiQuery<PageParams>,
) -> Result<Json<Page<User>>, ApiError> {
    let page = params.parse::<repo::ListKey>()?;
    let filter = repo::ListFilter {
        status: filter.status.map(Into::into),
        source: filter.source.map(Into::into),
        q: filter.q,
    };
    let scope = require.principal().scope();
    let rows = repo::list(&state.db, scope, &filter, page.after(), page.fetch_limit())
        .await
        .map_err(|error| match error {
            repo::Error::ContainsNul { field: "cursor" } => ApiError::invalid_cursor(),
            repo::Error::ContainsNul { .. } => {
                ApiError::malformed("`q` cannot contain the NUL character.")
            }
            other => ApiError::internal(&other),
        })?;
    let rows = rows
        .into_iter()
        .map(User::from_repo)
        .collect::<Result<Vec<_>, _>>()?;
    page.page(rows, |user| (user.display_name.clone(), user.id))
        .map(Json)
}

/// Read one user.
#[utoipa::path(
    get,
    path = "/api/v1/users/{user_id}",
    operation_id = "getUser",
    tag = "users",
    params(UserIdParam),
    responses(
        (status = 200, description = "The user", body = User),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = UserNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_user(
    require: Require<UserView>,
    State(state): State<AppState>,
    ApiPath(UserIdParam { user_id }): ApiPath<UserIdParam>,
) -> Result<Json<User>, ApiError> {
    let user = repo::get(&state.db, require.principal().scope(), user_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(user_not_found)?;
    Ok(Json(User::from_repo(user)?))
}

/// Create a user, `local` and `active`, and link the identities given to them.
#[utoipa::path(
    post,
    path = "/api/v1/users",
    operation_id = "createUser",
    tag = "users",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = NewUser,
    responses(
        (
            status = 201,
            description = "The user is created",
            body = User,
            headers(("Location" = String, description = "The user's path"))
        ),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = UserCreateForbidden),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 409, response = UserCreateConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn create_user(
    require: Require<UserManage>,
    entitlements: Entitlements,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<NewUser>,
) -> Result<Response, ApiError> {
    if body.identities.len() > MAX_NEW_IDENTITIES {
        return Err(invalid(&["identities"], code::IDENTITIES_TOO_MANY));
    }
    let scope = require.principal().scope();
    let now = state.clock.now();
    let mut tx = state.db.begin().await.map_err(|e| ApiError::internal(&e))?;
    entitlements.check_room(&mut tx, Limit::Users, 1).await?;
    let tz = match body.tz {
        Some(tz) => tz,
        None => {
            organizations::settings(&mut *tx, scope)
                .await
                .map_err(|error| ApiError::internal(&error))?
                .default_tz
        }
    };
    let new = repo::NewUser {
        display_name: body.display_name,
        given_name: body.given_name.unwrap_or_default(),
        family_name: body.family_name.unwrap_or_default(),
        email: body.email,
        locale: body.locale,
        tz,
        source: Source::Local,
        external_id: body.external_id,
    };
    let user = repo::create(&mut *tx, scope, &new, now)
        .await
        .map_err(user_error)?;
    for numbered in body.identities.iter().enumerate() {
        link(&mut tx, scope, user.id, numbered, now).await?;
    }
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    let location = format!("{USERS_PATH}/{}", user.id);
    let user = User::from_repo(user)?;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(user),
    )
        .into_response())
}

/// Link `identity`, the `index`th of the request's `identities`, to the new user `user_id`.
async fn link(
    tx: &mut Transaction<'static, Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    (index, identity): (usize, &NewUserIdentity),
    now: SystemTime,
) -> Result<(), ApiError> {
    let index = index.to_string();
    let invalid_at = |member, code| invalid(&["identities", &index, member], code);
    let provider = identity_providers::get_by_key(&mut **tx, scope, &identity.provider_key)
        .await
        .map_err(|error| match error {
            identity_providers::Error::ContainsNul { .. } => {
                invalid_at("provider_key", TEXT_CONTAINS_NUL)
            }
            other => ApiError::internal(&other),
        })?
        .ok_or_else(|| invalid_at("provider_key", code::IDENTITY_UNKNOWN_PROVIDER))?;
    let new = NewIdentity {
        user_id,
        provider_id: provider.id,
        subject: identity.subject.clone(),
        attributes: Map::new(),
    };
    match identities::link(&mut **tx, scope, &new, now).await {
        Ok(_) => Ok(()),
        Err(identities::Error::Taken) => Err(ApiError::new(
            StatusCode::CONFLICT,
            code::IDENTITY_TAKEN,
            "An identity to link is already linked to a user.",
        )),
        Err(identities::Error::SubjectEmpty) => {
            Err(invalid_at("subject", code::IDENTITY_SUBJECT_EMPTY))
        }
        Err(identities::Error::ContainsNul { .. }) => Err(invalid_at("subject", TEXT_CONTAINS_NUL)),
        Err(other) => Err(ApiError::internal(&other)),
    }
}

/// Change a user's names, email, locale, time zone or status. A patch naming nothing changes
/// nothing.
#[utoipa::path(
    patch,
    path = "/api/v1/users/{user_id}",
    operation_id = "updateUser",
    tag = "users",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(UserIdParam),
    request_body = UserPatch,
    responses(
        (status = 200, description = "The user after the change", body = User),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = UserWriteForbidden),
        (status = 404, response = UserNotFound),
        (status = 409, response = UserUpdateConflict),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_user(
    require: Require<UserManage>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiPath(UserIdParam { user_id }): ApiPath<UserIdParam>,
    ApiJson(body): ApiJson<UserPatch>,
) -> Result<Json<User>, ApiError> {
    let principal = require.principal();
    let scope = principal.scope();
    let now = state.clock.now();
    let mut tx = state.db.begin().await.map_err(|e| ApiError::internal(&e))?;
    let user = repo::get(&mut *tx, scope, user_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(user_not_found)?;
    refuse_patch(&body, &user, principal.user_id)?;
    let fields = body.fields();
    let mut user = if fields == repo::UserPatch::default() {
        user
    } else {
        repo::update(&mut *tx, scope, user_id, &fields, now)
            .await
            .map_err(user_error)?
    };
    if let Some(status) = body.status {
        let actor = client.actor(principal);
        user = repo::set_status(&mut *tx, scope, user_id, status.into(), now, actor)
            .await
            .map_err(user_error)?;
    }
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    Ok(Json(User::from_repo(user)?))
}

/// The route's own refusals of `patch` on `user` by the caller `caller` (a user id): a change
/// to a field an outside source owns (B6), then suspending yourself.
fn refuse_patch(patch: &UserPatch, user: &repo::User, caller: Uuid) -> Result<(), ApiError> {
    if user.source != Source::Local && patch.names_source_owned_field() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            code::USER_EXTERNALLY_MANAGED,
            "An outside system owns this user's names and email. Change them there.",
        ));
    }
    if patch.status == Some(UserStatus::Suspended) && user.id == caller {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            code::USER_CANNOT_SUSPEND_SELF,
            "You cannot suspend yourself.",
        ));
    }
    Ok(())
}

fn user_not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        USER_NOT_FOUND,
        "No user with this id exists in this organization.",
    )
}

/// 422 `request.invalid` with `code` at `path`.
fn invalid(path: &[&str], code: &'static str) -> ApiError {
    ApiError::invalid(vec![FieldError::at(path, code)])
}

/// The answer for a users repository error from a create or an update.
fn user_error(error: repo::Error) -> ApiError {
    use repo::Error as E;
    let conflict = |code, detail| ApiError::new(StatusCode::CONFLICT, code, detail);
    match error {
        E::NotFound => user_not_found(),
        E::DisplayNameEmpty => invalid(&["display_name"], NAME_EMPTY),
        E::EmailEmpty => invalid(&["email"], code::EMAIL_EMPTY),
        E::EmailInvalidCharacter => invalid(&["email"], code::EMAIL_INVALID_CHARACTER),
        E::ContainsNul { field } => invalid(&[field], TEXT_CONTAINS_NUL),
        E::TooLong { field, .. } => invalid(&[field], code::TEXT_TOO_LONG),
        E::TzUnknown => invalid(&["tz"], code::TZ_UNKNOWN),
        E::LocaleUnsupported => invalid(&["locale"], code::USER_LOCALE_UNSUPPORTED),
        E::ExternalIdTaken => conflict(
            code::USER_EXTERNAL_ID_TAKEN,
            "Another user of the organization has this external id.",
        ),
        E::EmailTaken => conflict(
            code::USER_EMAIL_TAKEN,
            "Another user of the organization has this email.",
        ),
        E::LastOrgAdmin => conflict(
            ROLE_ASSIGNMENT_LAST_ORG_ADMIN,
            "This is the last active Organization admin. Make someone else Organization admin first.",
        ),
        other @ (E::UnknownStoredValue { .. }
        | E::Time(_)
        | E::IsolationLevel { .. }
        | E::AdminCheck(_)
        | E::Database(_)) => ApiError::internal(&other),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn repository_errors_map_to_their_status_code_and_field() {
        let cases = [
            (repo::Error::NotFound, 404, "user.not_found", None),
            (
                repo::Error::TooLong {
                    field: "given_name",
                    max: 200,
                },
                422,
                "request.invalid",
                Some(("/given_name", "text.too_long")),
            ),
            (
                repo::Error::TzUnknown,
                422,
                "request.invalid",
                Some(("/tz", "tz.unknown")),
            ),
            (
                repo::Error::ContainsNul { field: "email" },
                422,
                "request.invalid",
                Some(("/email", "text.contains_nul")),
            ),
            (repo::Error::EmailTaken, 409, "user.email_taken", None),
            (
                repo::Error::ExternalIdTaken,
                409,
                "user.external_id_taken",
                None,
            ),
            (
                repo::Error::LastOrgAdmin,
                409,
                "role_assignment.last_org_admin",
                None,
            ),
            (
                repo::Error::IsolationLevel {
                    found: "serializable".into(),
                },
                500,
                "internal.error",
                None,
            ),
        ];
        for (error, status, code, field) in cases {
            let problem = user_error(error).problem();
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
    fn a_patch_tells_an_omitted_email_from_a_null_one() {
        let omitted: UserPatch = serde_json::from_value(json!({})).unwrap();
        assert_eq!(omitted.email, None);
        assert!(!omitted.names_source_owned_field());
        let cleared: UserPatch = serde_json::from_value(json!({ "email": null })).unwrap();
        assert_eq!(cleared.email, Some(None));
        assert!(cleared.names_source_owned_field());
        let status: UserPatch = serde_json::from_value(json!({ "status": "suspended" })).unwrap();
        assert!(!status.names_source_owned_field());
        assert_eq!(status.fields(), repo::UserPatch::default());
        assert!(serde_json::from_value::<UserPatch>(json!({ "tz": null })).is_err());
    }
}
