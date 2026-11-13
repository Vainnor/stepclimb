// SPDX-License-Identifier: AGPL-3.0-or-later
//! `GET /api/v1/me`: the signed-in user, their organization, their grants and the
//! organization's features (docs/specs/m0/03-users-and-identities.md B21, B22).
//!
//! The route needs a principal that is a user and no permission. Today the only such principal
//! is a browser session ([`Authenticated`]); the user and the organization are read with the
//! session's scope, so the answer can only be the caller's own rows. `grants` and `features` are
//! part of the schema and answered empty: spec 09 issue 6 fills `grants`, spec 10 issue 3 fills
//! `features`. The web app reads both only to show or hide UI; every route checks its own
//! permission and feature on the server.

use axum::{Json, extract::State};
use serde::Serialize;
use stepclimb_core::{Source, id::Uuid, organizations, users};
use utoipa::ToSchema;

use crate::{AppState, error::ApiError, roles::ScopeType, session::Authenticated};

/// The path of the route.
pub const ME_PATH: &str = "/api/v1/me";

/// What `GET /api/v1/me` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Me {
    pub user: MeUser,
    pub organization: MeOrganization,
    /// Each scope where the caller holds at least one effective permission, with those
    /// permissions sorted. Always empty until the server computes grants for this route.
    pub grants: Vec<MeGrant>,
    /// The keys of the organization's enabled features. Always empty until the server computes
    /// features for this route.
    pub features: Vec<String>,
}

/// The signed-in user.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct MeUser {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub display_name: String,
    /// `""` when the identity provider sent none.
    pub given_name: String,
    /// `""` when the identity provider sent none.
    pub family_name: String,
    #[schema(required = true)]
    pub email: Option<String>,
    /// `en` in M0.
    pub locale: String,
    /// An IANA time zone.
    pub tz: String,
    pub status: UserStatus,
    pub source: UserSource,
}

/// The signed-in user's organization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct MeOrganization {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub slug: String,
    pub name: String,
}

/// The permissions the caller holds at one scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct MeGrant {
    pub scope: GrantScope,
    /// Catalog keys, sorted.
    pub permissions: Vec<String>,
}

/// A scope a grant applies at: the organization, an account or a course, by id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct GrantScope {
    #[serde(rename = "type")]
    pub scope_type: ScopeType,
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
}

/// Whether a user may sign in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum UserStatus {
    Active,
    Suspended,
}

impl From<users::Status> for UserStatus {
    fn from(status: users::Status) -> Self {
        match status {
            users::Status::Active => Self::Active,
            users::Status::Suspended => Self::Suspended,
        }
    }
}

/// Where the user record came from: made by an admin (`local`), at sign-in through an identity
/// provider (`idp`), or by the sync API, SCIM or a connector, which own it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum UserSource {
    Local,
    Idp,
    Sync,
    Scim,
    Connector,
}

impl From<Source> for UserSource {
    fn from(source: Source) -> Self {
        match source {
            Source::Local => Self::Local,
            Source::Idp => Self::Idp,
            Source::Sync => Self::Sync,
            Source::Scim => Self::Scim,
            Source::Connector => Self::Connector,
        }
    }
}

impl From<users::User> for MeUser {
    fn from(user: users::User) -> Self {
        Self {
            id: user.id,
            display_name: user.display_name,
            given_name: user.given_name,
            family_name: user.family_name,
            email: user.email,
            locale: user.locale,
            tz: user.tz,
            status: user.status.into(),
            source: user.source.into(),
        }
    }
}

/// The session's user is not in the session's organization. Users are never deleted in M0 and
/// the session was found by joining its user within that organization, so this is a broken
/// invariant: a 500, never a 404 that would read as "no such user".
#[derive(Debug, thiserror::Error)]
#[error("the authenticated user is not in its organization")]
struct UserMissing;

/// Read the signed-in user.
#[utoipa::path(
    get,
    path = "/api/v1/me",
    operation_id = "getMe",
    tag = "me",
    responses(
        (status = 200, description = "The signed-in user and their organization", body = Me),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_me(
    principal: Authenticated,
    State(state): State<AppState>,
) -> Result<Json<Me>, ApiError> {
    let scope = principal.scope();
    let user = users::get(&state.db, scope, principal.user_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(|| ApiError::internal(&UserMissing))?;
    let organization = organizations::get(&state.db, scope)
        .await
        .map_err(|error| ApiError::internal(&error))?;
    Ok(Json(Me {
        user: user.into(),
        organization: MeOrganization {
            id: organization.id,
            slug: organization.slug,
            name: organization.name,
        },
        grants: Vec::new(),
        features: Vec::new(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The API's spellings are the stored ones, so a source or status added to the core cannot
    /// reach the API under another name.
    #[test]
    fn sources_and_statuses_serialize_as_their_stored_spelling() {
        for source in Source::ALL {
            let json = serde_json::to_value(UserSource::from(source)).unwrap();
            assert_eq!(json, source.as_str());
        }
        for status in [users::Status::Active, users::Status::Suspended] {
            let json = serde_json::to_value(UserStatus::from(status)).unwrap();
            assert_eq!(json, status.as_str());
        }
    }
}
