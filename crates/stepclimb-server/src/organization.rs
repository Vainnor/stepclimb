// SPDX-License-Identifier: AGPL-3.0-or-later
//! The caller's organization and its settings under `/api/v1/organization`
//! (docs/specs/m0/02-organizations-and-accounts.md B12 to B14 and API).
//!
//! A principal only ever sees its own organization: every route acts in the credential's scope,
//! and no route takes an organization id. Reading the organization needs a principal and no
//! permission; renaming it and reading or changing its settings need `organization.manage`,
//! which is grantable only at organization scope. The slug never changes through the API (Q3).
//!
//! The settings are the typed `stepclimb_core::OrgSettings` and `OrgSettingsPatch`. A patch
//! names only the keys it changes; each key it names that is no setting is listed in the 422 as
//! `settings.unknown_key`, and a known key whose value does not match its type is a 400
//! `request.malformed`, as for any body that does not match the schema (B2).
//!
//! `GET /api/v1/organization/entitlements` shows the organization's plan and what it allows, with
//! every catalog feature and limit (docs/specs/m0/10-entitlements-framework.md B12). It needs
//! `organization.manage`, and reads the plan through the request's [`RequestEntitlements`], the
//! same load every `Require<Feature>` of the request checks.

use std::collections::BTreeMap;

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use stepclimb_core::{
    OrgSettings as CoreSettings, OrgSettingsPatch as CorePatch,
    feature::{self, Limit},
    id::Uuid,
    org_plans::{OrgPlan, PlanSource as CoreSource},
    organizations,
    perm::OrgManage,
};
use utoipa::ToSchema;

use crate::{
    AppState,
    audit::ClientAddress,
    entitlements::Entitlements as RequestEntitlements,
    error::{ApiError, FieldError},
    extract::ApiJson,
    openapi::problem_responses,
    request_org,
    require::Require,
    roles::code::{NAME_EMPTY, NAME_TOO_LONG, TEXT_CONTAINS_NUL},
    session::Authenticated,
    users::code::TZ_UNKNOWN,
};

/// The path of the caller's organization.
pub const ORGANIZATION_PATH: &str = "/api/v1/organization";

/// The path of the caller's organization settings.
pub const SETTINGS_PATH: &str = "/api/v1/organization/settings";

/// The path of the caller's organization's entitlements.
pub const ENTITLEMENTS_PATH: &str = "/api/v1/organization/entitlements";

/// The codes this module produces.
pub mod code {
    /// In a 422's `errors`: the patch names a key that is no organization setting.
    pub const SETTINGS_UNKNOWN_KEY: &str = "settings.unknown_key";
}

problem_responses! {
    OrgWriteForbidden => "`permission.denied`: the caller does not hold `organization.manage` at organization scope. `auth.csrf_refused`: the request did not come from this site.",
    SettingsInvalid => "`request.invalid`: the patch breaks a rule; `errors` lists each one. `settings.unknown_key` marks a key that is no organization setting; `tz.unknown` a `default_tz` that is no IANA time zone.",
}

/// The caller's organization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Organization {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    /// Fixed when the organization is created.
    pub slug: String,
    pub name: String,
}

impl From<organizations::Organization> for Organization {
    fn from(org: organizations::Organization) -> Self {
        Self {
            id: org.id,
            slug: org.slug,
            name: org.name,
        }
    }
}

/// A change to the organization; an omitted member is left as it is. A member sent as `null`
/// is malformed: omit it instead.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OrganizationPatch {
    /// Trimmed; 1 to 200 characters.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = String)]
    pub name: Option<String>,
}

/// A member that, when present, must be a value: `null` fails the parse (400), and only an
/// omitted member reads as `None`, through `#[serde(default)]`.
pub(crate) fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Every organization setting with its effective value: the stored one, or the setting's
/// default when none is stored.
//
// The document's form of `stepclimb_core::OrgSettings`, which the routes serialize;
// `tests/organization.rs` checks the core type's answer against this schema.
#[derive(ToSchema)]
pub struct OrgSettings {
    /// The IANA time zone a user created without one gets; `UTC` unless set.
    pub default_tz: String,
}

/// A partial change to the organization settings: one optional member per setting; only the
/// members present change. A member that is no setting is refused.
//
// The document's form of `stepclimb_core::OrgSettingsPatch`, whose `deny_unknown_fields` this
// repeats so the document says `additionalProperties: false`; `tests/organization.rs` checks
// it declares the same members as `OrgSettings`.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OrgSettingsPatch {
    /// An IANA time zone, matched in any letter case and stored in the database's spelling.
    #[serde(default)]
    #[schema(value_type = String)]
    pub default_tz: Option<String>,
}

/// Read the caller's organization.
#[utoipa::path(
    get,
    path = "/api/v1/organization",
    operation_id = "getOrganization",
    tag = "organization",
    responses(
        (status = 200, description = "The caller's organization", body = Organization),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_organization(
    principal: Authenticated,
    State(state): State<AppState>,
) -> Result<Json<Organization>, ApiError> {
    let org = organizations::get(&state.db, principal.scope())
        .await
        .map_err(org_error)?;
    Ok(Json(org.into()))
}

/// Rename the caller's organization. A patch without `name` changes nothing.
#[utoipa::path(
    patch,
    path = "/api/v1/organization",
    operation_id = "updateOrganization",
    tag = "organization",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = OrganizationPatch,
    responses(
        (status = 200, description = "The organization after the change", body = Organization),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = OrgWriteForbidden),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = crate::openapi::UnprocessableEntity),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_organization(
    require: Require<OrgManage>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<OrganizationPatch>,
) -> Result<Json<Organization>, ApiError> {
    let scope = require.principal().scope();
    let org = match body.name {
        Some(name) => {
            let actor = client.actor(require.principal());
            organizations::rename(&state.db, scope, &name, state.clock.now(), actor).await
        }
        None => organizations::get(&state.db, scope).await,
    }
    .map_err(org_error)?;
    Ok(Json(org.into()))
}

/// Read the organization settings, every key with its effective value.
#[utoipa::path(
    get,
    path = "/api/v1/organization/settings",
    operation_id = "getOrganizationSettings",
    tag = "organization",
    responses(
        (status = 200, description = "Every setting with its effective value", body = OrgSettings),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_settings(
    require: Require<OrgManage>,
    State(state): State<AppState>,
) -> Result<Json<CoreSettings>, ApiError> {
    let settings = organizations::settings(&state.db, require.principal().scope())
        .await
        .map_err(org_error)?;
    Ok(Json(settings))
}

/// Change the settings the patch names; the others keep their value. A patch naming no setting
/// writes nothing.
#[utoipa::path(
    patch,
    path = "/api/v1/organization/settings",
    operation_id = "updateOrganizationSettings",
    tag = "organization",
    extensions(("x-stepclimb-mutates" = json!(true))),
    request_body = OrgSettingsPatch,
    responses(
        (status = 200, description = "Every setting after the change", body = OrgSettings),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = OrgWriteForbidden),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 413, response = crate::openapi::PayloadTooLarge),
        (status = 415, response = crate::openapi::UnsupportedMediaType),
        (status = 422, response = SettingsInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn update_settings(
    require: Require<OrgManage>,
    client: ClientAddress,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<Map<String, Value>>,
) -> Result<Json<CoreSettings>, ApiError> {
    let scope = require.principal().scope();
    let settings = match parse_patch(body)? {
        Some(patch) => {
            let actor = client.actor(require.principal());
            organizations::update_settings(&state.db, scope, &patch, state.clock.now(), actor).await
        }
        None => organizations::settings(&state.db, scope).await,
    }
    .map_err(org_error)?;
    Ok(Json(settings))
}

/// The organization's plan and what it allows: every feature and limit of the server's catalogs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[schema(as = Entitlements)]
pub struct OrgEntitlements {
    pub plan: PlanRef,
    /// Every catalog feature, by key, with whether the plan includes it.
    pub features: BTreeMap<String, bool>,
    /// Every catalog limit, by key, with the most units the plan allows, or `null` for no limit.
    pub limits: BTreeMap<String, Option<u64>>,
}

/// Which plan the organization runs on and how it got it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct PlanRef {
    /// The plan's key; `unlimited` for a self-hosted organization.
    pub key: String,
    pub source: PlanSource,
}

/// How the organization got its plan: a subscription (`stripe`), granted by the operator
/// (`grant`), the non-profit plan (`nonprofit`), or the self-hosted `unlimited` plan
/// (`selfhost`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PlanSource {
    Stripe,
    Grant,
    Nonprofit,
    Selfhost,
}

impl From<CoreSource> for PlanSource {
    fn from(source: CoreSource) -> Self {
        match source {
            CoreSource::Stripe => Self::Stripe,
            CoreSource::Grant => Self::Grant,
            CoreSource::Nonprofit => Self::Nonprofit,
            CoreSource::Selfhost => Self::Selfhost,
        }
    }
}

impl From<&OrgPlan> for OrgEntitlements {
    /// Lists [`feature::ALL`] and [`Limit::ALL`], not the plan row's keys: a key the catalog no
    /// longer has is left out, and one the row lacks shows as the plan reads it (B7).
    fn from(plan: &OrgPlan) -> Self {
        let rules = &plan.entitlements;
        Self {
            plan: PlanRef {
                key: plan.plan.clone(),
                source: plan.source.into(),
            },
            features: feature::ALL
                .iter()
                .map(|key| ((*key).to_owned(), rules.has_key(key)))
                .collect(),
            limits: Limit::ALL
                .iter()
                .map(|limit| (limit.key().to_owned(), rules.limit(*limit)))
                .collect(),
        }
    }
}

/// Read the organization's plan, every catalog feature with whether the plan includes it, and
/// every catalog limit with its value.
#[utoipa::path(
    get,
    path = "/api/v1/organization/entitlements",
    operation_id = "getOrganizationEntitlements",
    tag = "organization",
    responses(
        (status = 200, description = "The plan and what it allows", body = OrgEntitlements),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = crate::openapi::PermissionDenied),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn get_entitlements(
    _: Require<OrgManage>,
    entitlements: RequestEntitlements,
) -> Json<OrgEntitlements> {
    Json(entitlements.plan().into())
}

/// The most unknown keys a 422 lists, so a large body cannot make an equally large answer.
pub const MAX_UNKNOWN_KEYS: usize = 20;

const MALFORMED_PATCH: &str = "The request body is not valid JSON for this operation.";

/// The patch in `body`, `None` when it names no key. In B2's order: 400 when a member is
/// `null` (no setting is nullable; omit a member to leave it as it is), then 422 listing the
/// first [`MAX_UNKNOWN_KEYS`] keys that are no setting, then 400 when a known key's value does
/// not match its type.
fn parse_patch(body: Map<String, Value>) -> Result<Option<CorePatch>, ApiError> {
    if body.is_empty() {
        return Ok(None);
    }
    if body.values().any(Value::is_null) {
        return Err(ApiError::malformed(MALFORMED_PATCH));
    }
    let unknown: Vec<FieldError> = body
        .keys()
        .filter(|key| !CorePatch::knows(key))
        .take(MAX_UNKNOWN_KEYS)
        .map(|key| FieldError::at(&[key], code::SETTINGS_UNKNOWN_KEY))
        .collect();
    if !unknown.is_empty() {
        return Err(ApiError::invalid(unknown));
    }
    serde_json::from_value(Value::Object(body))
        .map(Some)
        .map_err(|_| ApiError::malformed(MALFORMED_PATCH))
}

/// The answer for an organizations repository error.
fn org_error(error: organizations::Error) -> ApiError {
    use organizations::Error as E;
    let invalid = |code| ApiError::invalid(vec![FieldError::at(&["name"], code)]);
    match error {
        E::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            request_org::code::ORG_NOT_FOUND,
            "The request's organization cannot be determined.",
        ),
        E::NameEmpty => invalid(NAME_EMPTY),
        E::NameTooLong => invalid(NAME_TOO_LONG),
        E::NameContainsNul => invalid(TEXT_CONTAINS_NUL),
        E::DefaultTzUnknown => ApiError::invalid(vec![FieldError::at(&["default_tz"], TZ_UNKNOWN)]),
        other @ (E::SlugInvalid
        | E::SlugReserved
        | E::SlugTaken
        | E::StoredSettings(_)
        | E::Plan(_)
        | E::OrgPlan(_)
        | E::Time(_)
        | E::Database(_)) => ApiError::internal(&other),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The API's spellings are the stored ones, so a source added to the core cannot reach the
    /// API under another name.
    #[test]
    fn plan_sources_serialize_as_their_stored_spelling() {
        for source in CoreSource::ALL {
            let json = serde_json::to_value(PlanSource::from(source)).unwrap();
            assert_eq!(json, source.as_str());
        }
    }

    #[test]
    fn a_patch_lists_every_unknown_key() {
        let body: Map<String, Value> = serde_json::from_str(r#"{"zeta": 1, "a/b": true}"#).unwrap();
        let problem = parse_patch(body).unwrap_err().problem();
        assert_eq!(problem.status, 422);
        assert_eq!(problem.code, "request.invalid");
        let fields: Vec<_> = problem
            .errors
            .iter()
            .map(|e| (e.field.as_str(), e.code.as_str()))
            .collect();
        assert_eq!(
            fields,
            [
                ("/a~1b", "settings.unknown_key"),
                ("/zeta", "settings.unknown_key")
            ]
        );
        assert_eq!(parse_patch(Map::new()).unwrap(), None);
    }

    #[test]
    fn a_patch_lists_at_most_twenty_unknown_keys() {
        let body: Map<String, Value> = (0..25).map(|i| (format!("k{i:02}"), json!(1))).collect();
        let problem = parse_patch(body).unwrap_err().problem();
        assert_eq!(problem.status, 422);
        let fields: Vec<_> = problem.errors.iter().map(|e| e.field.as_str()).collect();
        let first: Vec<String> = (0..MAX_UNKNOWN_KEYS).map(|i| format!("/k{i:02}")).collect();
        assert_eq!(fields, first);
    }

    #[test]
    fn a_null_member_is_malformed_whatever_its_key() {
        for body in [json!({ "no_such_key": null }), json!({ "a": 1, "b": null })] {
            let Value::Object(body) = body else {
                unreachable!()
            };
            let problem = parse_patch(body).unwrap_err().problem();
            assert_eq!(
                (problem.status, problem.code.as_str()),
                (400, "request.malformed")
            );
        }
    }

    #[test]
    fn a_rename_patch_refuses_null_and_reads_an_omitted_name_as_none() {
        assert!(serde_json::from_value::<OrganizationPatch>(json!({ "name": null })).is_err());
        let omitted: OrganizationPatch = serde_json::from_value(json!({})).unwrap();
        assert_eq!(omitted.name, None);
        let named: OrganizationPatch = serde_json::from_value(json!({ "name": "A" })).unwrap();
        assert_eq!(named.name.as_deref(), Some("A"));
    }

    #[test]
    fn repository_errors_map_to_their_status_code_and_field() {
        let cases = [
            (organizations::Error::NotFound, 404, "org.not_found", None),
            (
                organizations::Error::NameEmpty,
                422,
                "request.invalid",
                Some(("/name", "name.empty")),
            ),
            (
                organizations::Error::NameTooLong,
                422,
                "request.invalid",
                Some(("/name", "name.too_long")),
            ),
            (
                organizations::Error::NameContainsNul,
                422,
                "request.invalid",
                Some(("/name", "text.contains_nul")),
            ),
            (
                organizations::Error::DefaultTzUnknown,
                422,
                "request.invalid",
                Some(("/default_tz", "tz.unknown")),
            ),
            (organizations::Error::SlugTaken, 500, "internal.error", None),
        ];
        for (error, status, code, field) in cases {
            let problem = org_error(error).problem();
            assert_eq!((problem.status, problem.code.as_str()), (status, code));
            let fields: Vec<_> = problem
                .errors
                .iter()
                .map(|e| (e.field.as_str(), e.code.as_str()))
                .collect();
            assert_eq!(fields, field.into_iter().collect::<Vec<_>>(), "{code}");
        }
    }
}
