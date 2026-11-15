// SPDX-License-Identifier: AGPL-3.0-or-later
//! `GET /api/v1/me`: the signed-in user, their organization, their grants and the
//! organization's features (docs/specs/m0/03-users-and-identities.md B21, B22), the user's
//! appearance choice and the organization's default appearance (docs/specs/m0/11-app-shell.md
//! B21, Q6).
//!
//! The route needs a principal that is a user and no permission ([`AuthOnly`]). Today the only
//! such principal is a browser session; the user and the organization are read with the
//! session's scope, so the answer can only be the caller's own rows. `grants` are the grants
//! every `Require` of the request checks (spec 09 B16). `features` are the catalog features the
//! organization's plan includes, from the same per-request load every `Require<Feature>` checks
//! (spec 10 B12). The web app reads both only to show or hide UI; every route checks its own
//! permission and feature on the server.

use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use stepclimb_core::{
    Source, appearance, feature,
    grants::{Grants, ScopeId},
    id::Uuid,
    organizations,
    plans::PlanEntitlements,
    users,
};
use utoipa::ToSchema;

use crate::{
    AppState,
    appearance::{ThemeChoice, UserAppearance},
    branding,
    entitlements::Entitlements,
    error::ApiError,
    organization::Organization,
    require::AuthOnly,
    roles::ScopeType,
};

/// The path of the route.
pub const ME_PATH: &str = "/api/v1/me";

/// What `GET /api/v1/me` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Me {
    pub user: MeUser,
    pub organization: Organization,
    /// Each scope where the caller holds at least one effective permission, with those
    /// permissions sorted: the organization first, then accounts by id. A permission held at a
    /// scope also takes effect in every account inside it, which are not listed again.
    pub grants: Vec<MeGrant>,
    /// The keys of the catalog features the organization's plan includes, sorted.
    pub features: Vec<String>,
    /// The theme and density the user chose (docs/specs/m0/11-app-shell.md B21, Q6).
    pub appearance: UserAppearance,
    /// The organization's default appearance: the theme the user's pages take while
    /// `appearance.theme` is `null`.
    pub default_appearance: ThemeChoice,
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

/// The permissions the caller holds at one scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct MeGrant {
    pub scope: GrantScope,
    /// Catalog keys, sorted.
    pub permissions: Vec<String>,
}

/// A scope a grant or a role assignment applies at: the organization, an account or a course,
/// by id. An organization scope's `id` is the organization's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct GrantScope {
    #[serde(rename = "type")]
    pub scope_type: ScopeType,
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
}

/// Whether a user may sign in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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
    auth: AuthOnly,
    entitlements: Entitlements,
    State(state): State<AppState>,
) -> Result<Json<Me>, ApiError> {
    let principal = auth.principal();
    let scope = principal.scope();
    let user = users::get(&state.db, scope, principal.user_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(|| ApiError::internal(&UserMissing))?;
    let organization = organizations::get(&state.db, scope)
        .await
        .map_err(|error| ApiError::internal(&error))?;
    let appearance = appearance::get(&state.db, scope, principal.user_id)
        .await
        .map_err(|error| ApiError::internal(&error))?;
    let default_appearance = branding::default_appearance(&state, scope).await;
    Ok(Json(Me {
        appearance: appearance.into(),
        default_appearance: default_appearance.into(),
        user: user.into(),
        organization: organization.into(),
        grants: me_grants(auth.grants()),
        features: enabled_features(&entitlements.plan().entitlements),
    }))
}

/// The keys of [`feature::ALL`] the plan includes, sorted. A feature only a test crate defines
/// is in no catalog and never listed, even on the `unlimited` plan.
fn enabled_features(plan: &PlanEntitlements) -> Vec<String> {
    let mut keys: Vec<String> = feature::ALL
        .iter()
        .filter(|key| plan.has_key(key))
        .map(|key| (*key).to_owned())
        .collect();
    keys.sort_unstable();
    keys
}

/// The grants as `GET /api/v1/me` lists them.
fn me_grants(grants: &Grants) -> Vec<MeGrant> {
    grants
        .by_scope()
        .map(|(scope, keys)| {
            let (scope_type, id) = match scope {
                ScopeId::Organization(id) => (ScopeType::Organization, id),
                ScopeId::Account(id) => (ScopeType::Account, id),
            };
            MeGrant {
                scope: GrantScope { scope_type, id },
                permissions: keys.into_iter().map(str::to_owned).collect(),
            }
        })
        .collect()
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
        for status in users::Status::ALL {
            let json = serde_json::to_value(UserStatus::from(status)).unwrap();
            assert_eq!(json, status.as_str());
        }
    }
}
